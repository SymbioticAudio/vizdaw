//! DAWproject XML (spec §6.5): a bounded element-tree reader, strict number
//! parsing and an escaping, deterministic writer.
//!
//! The reader takes the whole text, already bounded by the package reader,
//! and refuses a hostile one before it costs memory or time: any document
//! type (so no entity is ever declared or expanded), any entity other than
//! the five predefined ones and character references, an encoding other than
//! UTF-8, nesting deeper than [`PackageLimits::max_depth`], more elements than
//! [`PackageLimits::max_nodes`], a tree larger than
//! [`PackageLimits::max_tree_bytes`] (its arena and nodes, counted as they
//! grow), more than [`MAX_XML_ATTRS`] attributes on an element, a repeated
//! attribute, an attribute value over [`MAX_TEXT_BYTES`] and broken markup.
//! Element and attribute names over [`MAX_XML_NAME_BYTES`] are refused as
//! broken by a pass over the text before the parser sees it (the parser
//! copies open elements' names). It never recurses. Beyond the text and the
//! tree it holds one element's pending text (at most [`MAX_TEXT_BYTES`]) and
//! the parser's copy of the open elements' names (at most 256 KiB).
//!
//! The reader records what it leaves out instead of reporting it: a text cut
//! at [`MAX_TEXT_BYTES`] ([`Tree::text_cut`]) and text dropped from elements
//! that have child elements ([`Tree::dropped_text`]). The mappers (Tasks 4
//! and 10) turn these into report lines.
//!
//! It accepts what the parser accepts beyond that: `<` in attribute values,
//! control characters such as NUL and U+FFFE in text and values, and names
//! that are not XML names. Consumers must sanitize every text and value
//! (`text::sanitize`) before showing or storing it.

use crate::IoError;
use crate::limits::{
    MAX_NUMBER_CHARS, MAX_TEXT_BYTES, MAX_XML_ATTRS, MAX_XML_NAME_BYTES, PackageLimits,
};
use crate::zip::damaged;
use quick_xml::XmlVersion;
use quick_xml::errors::Error as XmlError;
use quick_xml::escape::EscapeError;
use quick_xml::events::attributes::AttrError;
use quick_xml::events::{BytesStart, Event};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};

/// Parser events between two looks at the cancel flag.
const CHECK_EVERY: u64 = 65_536;
/// An attribute's raw text longer than this many times [`MAX_TEXT_BYTES`]
/// is refused before it is unescaped: every escape but a zero-padded
/// character reference (which no writer makes) takes at most 6 bytes per
/// byte it yields.
const RAW_ATTR_FACTOR: usize = 8;
/// No node, child or text.
const NONE: u32 = u32::MAX;
/// Set in [`Node::text`] when the text was cut at [`MAX_TEXT_BYTES`].
const CUT: u32 = 1 << 31;

const DOCTYPE: &str = "it declares a document type";
const ENTITY: &str = "it uses an unknown entity";
const NOT_UTF8: &str = "it is not UTF-8";
const TOO_DEEP: &str = "it is nested too deeply";
const TOO_MANY: &str = "it has too many parts";
const TOO_MANY_ATTRS: &str = "an element has too many attributes";
const REPEATED_ATTR: &str = "an element repeats an attribute";
const LONG_ATTR: &str = "an attribute is longer than 64 KiB";
const BROKEN: &str = "its XML is broken";

/// One element of a [`Tree`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeId(u32);

/// A run of the arena.
#[derive(Debug, Clone, Copy)]
struct Span {
    start: u32,
    len: u32,
}

/// An element: its name, first attribute, first child, next sibling and text.
/// Its attributes run up to the next element's first attribute.
#[derive(Debug, Clone, Copy)]
struct Node {
    name_start: u32,
    name_len: u32,
    first_attr: u32,
    first_child: u32,
    next_sibling: u32,
    /// Index into `Tree::texts`, with [`CUT`] if it was cut; or [`NONE`].
    text: u32,
}

#[derive(Debug, Clone, Copy)]
struct AttrRec {
    name: Span,
    value: Span,
}

/// An element tree: names and attribute values in one string arena; text kept
/// only up to MAX_TEXT_BYTES per element (MetaData's fields).
///
/// Only an element without child elements keeps its text, so indentation
/// between elements costs nothing. CDATA is text; comments and processing
/// instructions are skipped. Names keep their prefixes as written.
#[derive(Debug)]
pub struct Tree {
    arena: String,
    nodes: Vec<Node>,
    attrs: Vec<AttrRec>,
    texts: Vec<Span>,
    dropped_text: usize,
}

/// One element open during the parse.
#[derive(Debug, Clone, Copy)]
struct Frame {
    node: u32,
    last_child: u32,
}

/// Bytes the tree holds, against its limit.
struct Budget {
    used: usize,
    max: usize,
}

/// Something that grows in whole elements of `ITEM` bytes.
trait Grows {
    const ITEM: usize;
    fn len_now(&self) -> usize;
    fn cap_now(&self) -> usize;
    fn try_grow(&mut self, extra: usize) -> bool;
}

impl<T> Grows for Vec<T> {
    const ITEM: usize = size_of::<T>();
    fn len_now(&self) -> usize {
        self.len()
    }
    fn cap_now(&self) -> usize {
        self.capacity()
    }
    fn try_grow(&mut self, extra: usize) -> bool {
        self.try_reserve_exact(extra).is_ok()
    }
}

impl Grows for String {
    const ITEM: usize = 1;
    fn len_now(&self) -> usize {
        self.len()
    }
    fn cap_now(&self) -> usize {
        self.capacity()
    }
    fn try_grow(&mut self, extra: usize) -> bool {
        self.try_reserve_exact(extra).is_ok()
    }
}

impl Budget {
    /// Makes room for `extra` more items in `v`, growing it (doubling, but
    /// never past the budget) only when it is full.
    ///
    /// Growing moves the items to a new buffer, so for a moment both are
    /// held: the old buffer and the new one are charged together, and the
    /// tree's bytes never pass the budget, not even while a table grows.
    ///
    /// An empty table is sized at once for `first` items (what the pass over
    /// the text says it will hold), so a large table is not copied as it
    /// grows; it is sized only when first needed, so a file refused early
    /// costs nothing.
    fn room<G: Grows>(&mut self, v: &mut G, extra: usize, first: usize) -> Result<(), IoError> {
        let item = G::ITEM.max(1);
        let need = v
            .len_now()
            .checked_add(extra)
            .ok_or_else(|| damaged(TOO_MANY))?;
        let cap = v.cap_now();
        if need <= cap {
            return Ok(());
        }
        // `used` already holds the old buffer; the new one comes on top.
        let most = self.max.saturating_sub(self.used) / item;
        if need > most {
            return Err(damaged(TOO_MANY));
        }
        let grown = if cap == 0 {
            first
        } else {
            cap.saturating_mul(2)
        };
        let want = need.max(grown).max(64).min(most);
        let others = self.used.saturating_sub(cap.saturating_mul(item));
        if !v.try_grow(want.saturating_sub(v.len_now())) {
            return Err(damaged(TOO_MANY));
        }
        self.used = others.saturating_add(v.cap_now().saturating_mul(item));
        Ok(())
    }
}

/// What a pass over the text says the tree will hold at most (comments
/// can only make these larger; CDATA, and text split by comments, can make
/// the texts larger than this), so the tables are sized once, at their
/// first use, and seldom grow.
#[derive(Debug, Default, Clone, Copy)]
struct Bounds {
    /// Start tags.
    elements: usize,
    /// Attribute names.
    attrs: usize,
    /// Texts of elements, the runs after a start tag that are not all
    /// whitespace.
    texts: usize,
    /// Element and attribute names, attribute values as written (unescaping
    /// only shortens them; a value over [`MAX_TEXT_BYTES`] is refused, so
    /// each counts at most that) and those texts, each at most
    /// [`MAX_TEXT_BYTES`].
    arena: usize,
}

/// `n` as an arena or table index.
fn index(n: usize) -> Result<u32, IoError> {
    u32::try_from(n)
        .ok()
        .filter(|&i| i != NONE)
        .ok_or_else(|| damaged(TOO_MANY))
}

fn is_xml_space(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
}

/// The refusal for a parser error.
fn from_xml(e: &XmlError) -> IoError {
    match e {
        XmlError::Escape(EscapeError::UnrecognizedEntity(..)) => damaged(ENTITY),
        XmlError::Encoding(_) => damaged(NOT_UTF8),
        XmlError::InvalidAttr(AttrError::Duplicated(..)) => damaged(REPEATED_ATTR),
        _ => damaged(BROKEN),
    }
}

/// The parse under way.
struct Builder {
    tree: Tree,
    budget: Budget,
    /// What the tables are first sized for.
    bounds: Bounds,
    stack: Vec<Frame>,
    /// The innermost element's text while it has no child element.
    pending: String,
    /// Whether the innermost element has had a child element.
    has_child: bool,
    /// Whether `pending` was cut at [`MAX_TEXT_BYTES`].
    pending_cut: bool,
    /// Whether a run of dropped non-whitespace text is under way.
    dropping: bool,
    root_done: bool,
    max_depth: usize,
    max_nodes: usize,
}

impl Builder {
    /// Pushes `s` into the arena.
    fn intern(&mut self, s: &str) -> Result<Span, IoError> {
        self.budget
            .room(&mut self.tree.arena, s.len(), self.bounds.arena)?;
        let start = index(self.tree.arena.len())?;
        let len = index(s.len())?;
        self.tree.arena.push_str(s);
        Ok(Span { start, len })
    }

    fn open(&mut self, e: &BytesStart<'_>) -> Result<(), IoError> {
        if self.root_done {
            return Err(damaged(BROKEN));
        }
        if self.stack.len() >= self.max_depth {
            return Err(damaged(TOO_DEEP));
        }
        if self.tree.nodes.len() >= self.max_nodes {
            return Err(damaged(TOO_MANY));
        }
        let id = index(self.tree.nodes.len())?;
        let name = self.intern(e.name().as_ref())?;
        let first_attr = index(self.tree.attrs.len())?;
        for (count, a) in e.attributes().enumerate() {
            if count >= MAX_XML_ATTRS {
                return Err(damaged(TOO_MANY_ATTRS));
            }
            let a = a.map_err(|e| from_xml(&XmlError::InvalidAttr(e)))?;
            if a.value.len() > MAX_TEXT_BYTES.saturating_mul(RAW_ATTR_FACTOR) {
                return Err(damaged(LONG_ATTR));
            }
            let value = a
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(|e| from_xml(&e))?;
            if value.len() > MAX_TEXT_BYTES {
                return Err(damaged(LONG_ATTR));
            }
            let name = self.intern(a.key.as_ref())?;
            let value = self.intern(&value)?;
            self.budget
                .room(&mut self.tree.attrs, 1, self.bounds.attrs)?;
            self.tree.attrs.push(AttrRec { name, value });
        }
        self.budget
            .room(&mut self.tree.nodes, 1, self.bounds.elements)?;
        self.tree.nodes.push(Node {
            name_start: name.start,
            name_len: name.len,
            first_attr,
            first_child: NONE,
            next_sibling: NONE,
            text: NONE,
        });
        if let Some(parent) = self.stack.last_mut() {
            let prev = std::mem::replace(&mut parent.last_child, id);
            let link = if prev == NONE {
                self.tree
                    .nodes
                    .get_mut(parent.node as usize)
                    .map(|n| &mut n.first_child)
            } else {
                self.tree
                    .nodes
                    .get_mut(prev as usize)
                    .map(|n| &mut n.next_sibling)
            };
            if let Some(link) = link {
                *link = id;
            }
        }
        self.stack.push(Frame {
            node: id,
            last_child: NONE,
        });
        // The parent's text so far is mixed content: dropped.
        if !is_xml_space(&self.pending) {
            self.tree.dropped_text = self.tree.dropped_text.saturating_add(1);
        }
        self.pending.clear();
        self.pending_cut = false;
        self.dropping = false;
        self.has_child = false;
        Ok(())
    }

    /// Closes the innermost element (the parser has matched its end tag).
    fn close(&mut self) -> Result<(), IoError> {
        let Some(frame) = self.stack.pop() else {
            return Err(damaged(BROKEN));
        };
        if !self.has_child && !self.pending.is_empty() {
            let text = std::mem::take(&mut self.pending);
            let span = self.intern(&text)?;
            self.pending = text;
            self.pending.clear();
            let at = index(self.tree.texts.len())?;
            if at >= CUT {
                return Err(damaged(TOO_MANY));
            }
            self.budget
                .room(&mut self.tree.texts, 1, self.bounds.texts)?;
            self.tree.texts.push(span);
            if let Some(n) = self.tree.nodes.get_mut(frame.node as usize) {
                n.text = if self.pending_cut { at | CUT } else { at };
            }
        }
        self.pending.clear();
        self.pending_cut = false;
        self.dropping = false;
        // The parent has had this child.
        self.has_child = true;
        if self.stack.is_empty() {
            self.root_done = true;
        }
        Ok(())
    }

    /// Text inside or around the root, as written (`eol`: with line ends
    /// still to normalise; a reference's character is not). Only what fits
    /// in `pending` is ever looked at past the first checks, so a huge text
    /// costs no copy.
    fn text(&mut self, s: &str, eol: bool) -> Result<(), IoError> {
        if self.stack.is_empty() {
            return if is_xml_space(s) {
                Ok(())
            } else {
                Err(damaged(BROKEN))
            };
        }
        if self.has_child {
            if !self.dropping && !is_xml_space(s) {
                self.dropping = true;
                self.tree.dropped_text = self.tree.dropped_text.saturating_add(1);
            }
            return Ok(());
        }
        if self.pending_cut {
            return Ok(());
        }
        let mut chars = s.chars().peekable();
        while let Some(mut c) = chars.next() {
            if eol && c == '\r' {
                chars.next_if_eq(&'\n');
                c = '\n';
            }
            if self.pending.len().saturating_add(c.len_utf8()) > MAX_TEXT_BYTES {
                self.pending_cut = true;
                break;
            }
            self.pending.push(c);
        }
        Ok(())
    }

    fn reference(&mut self, r: &quick_xml::events::BytesRef<'_>) -> Result<(), IoError> {
        if self.stack.is_empty() {
            return Err(damaged(BROKEN));
        }
        let c = match r.resolve_char_ref() {
            Ok(Some(c)) => c,
            Ok(None) => match &**r {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => return Err(damaged(ENTITY)),
            },
            Err(_) => return Err(damaged(BROKEN)),
        };
        let mut buf = [0u8; 4];
        self.text(c.encode_utf8(&mut buf), false)
    }
}

impl Tree {
    /// Parses `text` (a UTF-8 BOM is skipped), looking at `cancel` every
    /// 65 536 parser events. Each refusal is "This DAWproject file is
    /// damaged: <why>."
    pub fn parse(
        text: &[u8],
        limits: &PackageLimits,
        cancel: &AtomicBool,
    ) -> Result<Tree, IoError> {
        parse_observed(text, limits, cancel, &mut |_| {})
    }

    /// Test hook: [`Tree::parse`], calling `observe` with the number of
    /// events read so far at each look at the cancel flag, just before it.
    #[cfg(feature = "test-hooks")]
    pub fn debug_parse_observed(
        text: &[u8],
        limits: &PackageLimits,
        cancel: &AtomicBool,
        observe: &mut dyn FnMut(u64),
    ) -> Result<Tree, IoError> {
        parse_observed(text, limits, cancel, observe)
    }

    /// The root element.
    pub fn root(&self) -> NodeId {
        NodeId(0)
    }

    fn node(&self, n: NodeId) -> Option<&Node> {
        self.nodes.get(n.0 as usize)
    }

    fn span(&self, s: Span) -> &str {
        let start = s.start as usize;
        self.arena
            .get(start..start.saturating_add(s.len as usize))
            .unwrap_or_default()
    }

    /// The element's name, with any prefix.
    pub fn name(&self, n: NodeId) -> &str {
        self.node(n).map_or("", |node| {
            self.span(Span {
                start: node.name_start,
                len: node.name_len,
            })
        })
    }

    fn attr_range(&self, n: NodeId) -> &[AttrRec] {
        let Some(node) = self.node(n) else {
            return &[];
        };
        let end = self
            .nodes
            .get(n.0 as usize + 1)
            .map_or(self.attrs.len(), |next| next.first_attr as usize);
        self.attrs
            .get(node.first_attr as usize..end)
            .unwrap_or_default()
    }

    /// The value of the element's attribute `name` (as written, with any
    /// prefix), unescaped and normalised.
    pub fn attr(&self, n: NodeId, name: &str) -> Option<&str> {
        self.attr_range(n)
            .iter()
            .find(|a| self.span(a.name) == name)
            .map(|a| self.span(a.value))
    }

    /// The element's attributes as written, in order: name (with any
    /// prefix) and unescaped, normalised value.
    pub fn attrs(&self, n: NodeId) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.attr_range(n)
            .iter()
            .map(|a| (self.span(a.name), self.span(a.value)))
    }

    /// The element's child elements, in order.
    pub fn children(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let first = self.node(n).map_or(NONE, |node| node.first_child);
        std::iter::successors((first != NONE).then_some(NodeId(first)), |c| {
            self.node(*c)
                .map(|node| node.next_sibling)
                .filter(|&s| s != NONE)
                .map(NodeId)
        })
    }

    /// The element's text, if it has no child elements (at most
    /// [`MAX_TEXT_BYTES`]); otherwise empty.
    pub fn text(&self, n: NodeId) -> &str {
        self.node(n)
            .filter(|node| node.text != NONE)
            .and_then(|node| self.texts.get((node.text & !CUT) as usize))
            .map_or("", |s| self.span(*s))
    }

    /// Whether the element's text was cut at [`MAX_TEXT_BYTES`].
    pub fn text_cut(&self, n: NodeId) -> bool {
        self.node(n)
            .is_some_and(|node| node.text != NONE && node.text & CUT != 0)
    }

    /// How many runs of non-whitespace text were dropped because their
    /// element has child elements (mixed content). Whitespace is layout and
    /// is not counted.
    pub fn dropped_text(&self) -> usize {
        self.dropped_text
    }

    /// The bytes the tree holds (the capacity of its arena and tables), for
    /// callers that keep one budget across the tree and what they build
    /// from it.
    pub fn bytes(&self) -> usize {
        self.arena
            .capacity()
            .saturating_add(self.nodes.capacity().saturating_mul(size_of::<Node>()))
            .saturating_add(self.attrs.capacity().saturating_mul(size_of::<AttrRec>()))
            .saturating_add(self.texts.capacity().saturating_mul(size_of::<Span>()))
    }

    /// How many elements the tree holds (a parsed tree holds its root).
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
}

fn parse_observed(
    text: &[u8],
    limits: &PackageLimits,
    cancel: &AtomicBool,
    observe: &mut dyn FnMut(u64),
) -> Result<Tree, IoError> {
    observe(0);
    if cancel.load(Ordering::Relaxed) {
        return Err(IoError::Cancelled);
    }
    let text = text.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(text);
    let text = std::str::from_utf8(text).map_err(|_| damaged(NOT_UTF8))?;
    let Some(bounds) = scan(text.as_bytes()) else {
        return Err(damaged(BROKEN));
    };
    let mut reader = quick_xml::Reader::from_str(text);
    {
        let c = reader.config_mut();
        c.check_end_names = true;
        c.expand_empty_elements = false;
        c.allow_unmatched_ends = false;
        c.allow_dangling_amp = false;
        c.trim_text(false);
    }
    let mut b = Builder {
        tree: Tree {
            arena: String::new(),
            nodes: Vec::new(),
            attrs: Vec::new(),
            texts: Vec::new(),
            dropped_text: 0,
        },
        budget: Budget {
            used: 0,
            max: limits.max_tree_bytes,
        },
        bounds: Bounds {
            elements: bounds.elements.min(limits.max_nodes),
            ..bounds
        },
        stack: Vec::new(),
        pending: String::new(),
        has_child: false,
        pending_cut: false,
        dropping: false,
        root_done: false,
        max_depth: limits.max_depth,
        max_nodes: limits.max_nodes,
    };
    let mut events: u64 = 0;
    loop {
        if events > 0 && events % CHECK_EVERY == 0 {
            observe(events);
            if cancel.load(Ordering::Relaxed) {
                return Err(IoError::Cancelled);
            }
        }
        events = events.saturating_add(1);
        match reader.read_event().map_err(|e| from_xml(&e))? {
            Event::Start(e) => b.open(&e)?,
            Event::Empty(e) => {
                b.open(&e)?;
                b.close()?;
            }
            Event::End(_) => b.close()?,
            Event::Text(t) => b.text(&t, true)?,
            Event::CData(t) => {
                if b.stack.is_empty() {
                    return Err(damaged(BROKEN));
                }
                b.text(&t, true)?;
            }
            Event::GeneralRef(r) => b.reference(&r)?,
            // A declaration only as the very first thing.
            Event::Decl(_) if events > 1 => return Err(damaged(BROKEN)),
            Event::Decl(d) => match d.encoding() {
                None => {}
                Some(Ok(enc)) if enc.eq_ignore_ascii_case("utf-8") => {}
                Some(Ok(_)) => return Err(damaged(NOT_UTF8)),
                Some(Err(_)) => return Err(damaged(BROKEN)),
            },
            Event::DocType(_) => return Err(damaged(DOCTYPE)),
            Event::Comment(_) | Event::PI(_) => {}
            Event::Eof => break,
        }
    }
    if !b.root_done || !b.stack.is_empty() {
        return Err(damaged(BROKEN));
    }
    Ok(b.tree)
}

fn is_space_byte(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// Whether every element and attribute name in `b` is at most
/// [`MAX_XML_NAME_BYTES`], measured as the parser will see it: an element
/// name runs to whitespace or the tag's unquoted `>` (less a closing `/`),
/// with the [`Bounds`] of the tree; `None` if a name is too long. One pass,
/// no allocation. Names inside comments and CDATA are measured too (a
/// hostile-only false refusal); `<!` and `<?` markup is not a name.
fn scan(b: &[u8]) -> Option<Bounds> {
    let mut n = Bounds::default();
    let find = |from: usize, byte: u8| {
        b.get(from..)
            .and_then(|r| r.iter().position(|&c| c == byte))
            .map(|p| from.saturating_add(p))
    };
    let add = |x: &mut usize, by: usize| *x = x.saturating_add(by);
    let mut i = 0;
    while let Some(lt) = find(i, b'<') {
        i = lt.saturating_add(1);
        let end_tag = match b.get(i) {
            Some(b'!' | b'?') => continue,
            Some(b'/') => {
                i = i.saturating_add(1);
                true
            }
            _ => false,
        };
        // The element name, as the parser takes it (quotes and all).
        let start = i;
        loop {
            match b.get(i) {
                None => return (i.saturating_sub(start) <= MAX_XML_NAME_BYTES).then_some(n),
                Some(&c) if is_space_byte(c) || c == b'>' => break,
                Some(&q @ (b'"' | b'\'')) => match find(i.saturating_add(1), q) {
                    Some(end) => i = end.saturating_add(1),
                    None => i = b.len(),
                },
                Some(_) => i = i.saturating_add(1),
            }
            if i.saturating_sub(start) > MAX_XML_NAME_BYTES.saturating_add(1) {
                return None;
            }
        }
        let mut len = i.saturating_sub(start);
        if b.get(i) == Some(&b'>') && i > start && b.get(i.saturating_sub(1)) == Some(&b'/') {
            len = len.saturating_sub(1);
        }
        if len > MAX_XML_NAME_BYTES {
            return None;
        }
        if !end_tag {
            add(&mut n.elements, 1);
            add(&mut n.arena, len);
        }
        // The attributes, to the tag's end; values are skipped whole.
        loop {
            match b.get(i) {
                None => return Some(n),
                Some(b'>') => {
                    let empty = b.get(i.saturating_sub(1)) == Some(&b'/');
                    i = i.saturating_add(1);
                    if !end_tag && !empty {
                        // The element's text, if it is a leaf.
                        let to = find(i, b'<').unwrap_or(b.len());
                        let run = b.get(i..to).unwrap_or_default();
                        if !run.iter().all(|&c| is_space_byte(c)) {
                            add(&mut n.texts, 1);
                            add(&mut n.arena, run.len().min(MAX_TEXT_BYTES));
                        }
                    }
                    break;
                }
                Some(b'<') => break,
                Some(&q @ (b'"' | b'\'')) => match find(i.saturating_add(1), q) {
                    Some(end) => {
                        // A longer value is refused.
                        let len = end.saturating_sub(i).saturating_sub(1);
                        add(&mut n.arena, len.min(MAX_TEXT_BYTES));
                        i = end.saturating_add(1);
                    }
                    None => return Some(n),
                },
                Some(&c) if is_space_byte(c) || c == b'/' || c == b'=' => i = i.saturating_add(1),
                Some(_) => {
                    let start = i;
                    while b.get(i).is_some_and(|&c| {
                        !is_space_byte(c) && !matches!(c, b'/' | b'=' | b'>' | b'<' | b'"' | b'\'')
                    }) {
                        i = i.saturating_add(1);
                        if i.saturating_sub(start) > MAX_XML_NAME_BYTES {
                            return None;
                        }
                    }
                    add(&mut n.attrs, 1);
                    add(&mut n.arena, i.saturating_sub(start));
                }
            }
        }
    }
    Some(n)
}

/// The XSD `double` lexical form without its special values: an optional
/// sign, digits with an optional point (at least one digit), and an optional
/// exponent.
fn xsd_decimal(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let digits = |i: &mut usize| {
        let from = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i - from
    };
    let mut mantissa = digits(&mut i);
    if b.get(i) == Some(&b'.') {
        i += 1;
        mantissa += digits(&mut i);
    }
    if mantissa == 0 {
        return false;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return false;
        }
    }
    i == b.len()
}

/// `s` with XSD whitespace trimmed, if it is no longer than a number may be.
fn number_text(s: &str) -> Option<&str> {
    let s = s.trim_matches([' ', '\t', '\n', '\r']);
    (!s.is_empty() && s.len() <= MAX_NUMBER_CHARS).then_some(s)
}

/// An XSD `double`, finite, at most [`MAX_NUMBER_CHARS`] characters once
/// surrounding whitespace is trimmed; `None` for anything else (`NaN`,
/// `INF`, hex, separators, overflow to infinity).
pub fn double(s: &str) -> Option<f64> {
    let s = number_text(s)?;
    if !xsd_decimal(s) {
        return None;
    }
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// [`double`], or `inf`, `-inf`, `INF` or `-INF`.
pub fn double_or_inf(s: &str) -> Option<f64> {
    match number_text(s)? {
        "inf" | "INF" => Some(f64::INFINITY),
        "-inf" | "-INF" => Some(f64::NEG_INFINITY),
        _ => double(s),
    }
}

/// `[+-]?[0-9]+` in `i32`'s range, at most [`MAX_NUMBER_CHARS`] characters
/// once surrounding whitespace is trimmed.
pub fn int(s: &str) -> Option<i32> {
    let s = number_text(s)?;
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `true`, `false`, `1` or `0`, surrounding whitespace trimmed.
pub fn boolean(s: &str) -> Option<bool> {
    match s.trim_matches([' ', '\t', '\n', '\r']) {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// The shortest text that reads back as `v` exactly: Rust's `{}` for
/// 1e-7 ≤ |v| < 1e21, its `{:e}` outside that (so no number is longer than
/// [`MAX_NUMBER_CHARS`]); `-0` is written "0". Never NaN or infinity: a
/// non-finite `v` is a caller's bug and is written "0".
pub fn fmt_double(v: f64) -> String {
    debug_assert!(v.is_finite(), "fmt_double({v})");
    if !v.is_finite() || v == 0.0 {
        return "0".to_owned();
    }
    let a = v.abs();
    if (1e-7..1e21).contains(&a) {
        format!("{v}")
    } else {
        format!("{v:e}")
    }
}

/// An attribute value for [`XmlWriter`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Attr<'a> {
    /// Text, escaped.
    Str(&'a str),
    /// A number, by [`fmt_double`]. The caller checks it is finite first:
    /// a NaN or infinity is a bug (a debug assertion) and is written "0".
    F64(f64),
    /// An integer.
    I64(i64),
    /// `true` or `false`.
    Bool(bool),
}

/// Whether `c` cannot appear in XML at all (dropped by the writer): C0
/// controls other than tab, newline and CR, and U+FFFE and U+FFFF.
fn unwritable(c: char) -> bool {
    (c < ' ' && !matches!(c, '\t' | '\n' | '\r')) || matches!(c, '\u{FFFE}' | '\u{FFFF}')
}

fn escape_attr(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c if unwritable(c) => {}
            c => out.push(c),
        }
    }
}

fn escape_text(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c if unwritable(c) => {}
            c => out.push(c),
        }
    }
}

/// Writes XML deterministically: two spaces of indent per level, one element
/// per line, `/>` for empty elements, attributes in the order given, every
/// attribute value and text escaped. Names are the caller's and are written
/// as given.
#[derive(Debug)]
pub struct XmlWriter {
    out: String,
    open: Vec<String>,
}

impl Default for XmlWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl XmlWriter {
    /// A document holding only its declaration line.
    pub fn new() -> Self {
        Self {
            out: "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n".to_owned(),
            open: Vec::new(),
        }
    }

    fn start_tag(&mut self, name: &str, attrs: &[(&str, Attr<'_>)]) {
        for _ in 0..self.open.len() {
            self.out.push_str("  ");
        }
        self.out.push('<');
        self.out.push_str(name);
        for (key, value) in attrs {
            self.out.push(' ');
            self.out.push_str(key);
            self.out.push_str("=\"");
            match value {
                Attr::Str(s) => escape_attr(&mut self.out, s),
                Attr::F64(v) => self.out.push_str(&fmt_double(*v)),
                Attr::I64(v) => {
                    let _ = write!(self.out, "{v}");
                }
                Attr::Bool(v) => self.out.push_str(if *v { "true" } else { "false" }),
            }
            self.out.push('"');
        }
    }

    /// Opens `name`; its children follow until [`XmlWriter::close`].
    pub fn open(&mut self, name: &str, attrs: &[(&str, Attr<'_>)]) {
        self.start_tag(name, attrs);
        self.out.push_str(">\n");
        self.open.push(name.to_owned());
    }

    /// An element with no children or text.
    pub fn empty(&mut self, name: &str, attrs: &[(&str, Attr<'_>)]) {
        self.start_tag(name, attrs);
        self.out.push_str("/>\n");
    }

    /// An element holding only `text`, on one line.
    pub fn text_element(&mut self, name: &str, text: &str) {
        self.start_tag(name, &[]);
        self.out.push('>');
        escape_text(&mut self.out, text);
        self.out.push_str("</");
        self.out.push_str(name);
        self.out.push_str(">\n");
    }

    /// Closes the innermost open element.
    pub fn close(&mut self) {
        debug_assert!(!self.open.is_empty(), "close with nothing open");
        if let Some(name) = self.open.pop() {
            for _ in 0..self.open.len() {
                self.out.push_str("  ");
            }
            self.out.push_str("</");
            self.out.push_str(&name);
            self.out.push_str(">\n");
        }
    }

    /// The document. Closes nothing: an element still open is a bug.
    pub fn finish(self) -> String {
        debug_assert!(self.open.is_empty(), "finish with {:?} open", self.open);
        self.out
    }
}
