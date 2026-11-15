//! The one way another format's mapping makes a [`DpImport`] (plan 6f
//! Ruling 10): audio tracks with their fader and pan, regions, automation
//! points, media, markers, the maps, the name and the song info. As a child
//! of `import` it builds the private plan directly, so the AAF mapping is
//! applied by 6d's `apply_tracks` and `arrange_tracks` like a DAWproject's.
//! An import built here is never trusted (it has no XML IDs) and has no
//! buses, sequences or plug-ins.

use super::maps::cut_to;
use super::timeline::{Arranged, AutoAt, Body, Item, MediaBook, RegionPlan, StripAt, media_plans};
use super::*;
use crate::dawproject::package::AudioRef;
use core_model::event::FadeShape;

/// An audio region to place, in the media item's frames.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RegionSpec {
    /// Its media item ([`PlanBuilder::media`]).
    pub(crate) media: usize,
    pub(crate) source_offset: u64,
    pub(crate) length: u64,
    pub(crate) fade_in: u64,
    pub(crate) fade_out: u64,
    pub(crate) fade_in_shape: FadeShape,
    pub(crate) fade_out_shape: FadeShape,
    /// Its gain in dB, within the model's −144 … +24.
    pub(crate) gain_db: f32,
    pub(crate) name: Option<String>,
    /// Its fade-in is a crossfade with the region it starts inside.
    pub(crate) crossfade: bool,
}

/// What a media item of the plan is: its rate, channels and frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MediaFacts {
    pub(crate) rate: u32,
    pub(crate) channels: u16,
    pub(crate) frames: u64,
}

/// A plan being built.
pub(crate) struct PlanBuilder {
    plan: Plan,
    items: Vec<Vec<Item>>,
    book: MediaBook,
}

impl PlanBuilder {
    /// An empty plan on `tempo` and `meter`, knowing the paths of
    /// `sources`' online items (offline items get paths apart from them).
    pub(crate) fn new(
        sources: &BTreeMap<usize, MediaSource>,
        tempo: TempoMap,
        meter: MeterMap,
    ) -> PlanBuilder {
        PlanBuilder {
            plan: Plan {
                name: String::new(),
                info: SongInfo::default(),
                tempo,
                meter,
                markers: Vec::new(),
                marker_tags: Vec::new(),
                buses: Vec::new(),
                master: Mix::default(),
                builtin: None,
                tracks: Vec::new(),
                arranged: Arranged::default(),
            },
            items: Vec::new(),
            book: MediaBook::new(sources),
        }
    }

    /// A new audio track (its own output path) whose strip has `gain_db`
    /// and `pan` (−1 … +1); its index.
    pub(crate) fn audio_track(&mut self, name: String, gain_db: f64, pan: f64) -> usize {
        self.plan.tracks.push(TrackPlan {
            dest_name: name.clone(),
            name,
            midi: None,
            audio: true,
            mix: Mix {
                gain_db,
                pan,
                ..Mix::default()
            },
            flags: None,
            tag: None,
            strip_tag: None,
            channel_name: None,
        });
        self.items.push(Vec::new());
        self.plan.tracks.len() - 1
    }

    /// Track `track`'s name.
    pub(crate) fn track_name(&self, track: usize) -> &str {
        self.plan.tracks.get(track).map_or("", |t| t.name.as_str())
    }

    /// The plan's media items for audio file `aref` (6d's [`media_plans`]:
    /// the converted items of `source`, else one offline item, listed).
    pub(crate) fn media(
        &mut self,
        aref: &AudioRef,
        source: Option<&MediaSource>,
        report: &mut Report,
    ) -> Vec<usize> {
        media_plans(&mut self.book, aref, source, report)
    }

    /// Media item `m`'s rate, channels and frames.
    pub(crate) fn media_facts(&self, m: usize) -> Option<MediaFacts> {
        self.book.media.get(m).map(|p| MediaFacts {
            rate: p.rate,
            channels: p.channels,
            frames: p.frames,
        })
    }

    fn push(&mut self, track: usize, tick: Tick, body: Body) {
        if let Some(v) = self.items.get_mut(track) {
            v.push(Item {
                tick,
                mute: false,
                body,
                tag: None,
            });
        }
    }

    /// A region on `track` at `tick`.
    pub(crate) fn region(&mut self, track: usize, tick: Tick, r: RegionSpec) {
        let body = Body::Region(RegionPlan {
            media: r.media,
            source_offset: r.source_offset,
            length: r.length,
            fade_in: r.fade_in,
            fade_out: r.fade_out,
            name: r.name,
            crossfade: r.crossfade,
            fade_in_shape: r.fade_in_shape,
            fade_out_shape: r.fade_out_shape,
            gain_db: r.gain_db,
        });
        self.push(track, tick, body);
    }

    /// A fader automation point (0 … 1, `fader_norm`'s scale) of `track`'s
    /// strip, on the track.
    pub(crate) fn fader_point(&mut self, track: usize, tick: Tick, value: f64) {
        let at = AutoAt::Fader(StripAt::Track(track));
        self.push(track, tick, Body::Auto { at, value });
    }

    /// A pan automation point (0 left, 0.5 centre, 1 right) of `track`'s
    /// strip, on the track.
    pub(crate) fn pan_point(&mut self, track: usize, tick: Tick, value: f64) {
        let at = AutoAt::Pan(StripAt::Track(track));
        self.push(track, tick, Body::Auto { at, value });
    }

    /// A marker.
    pub(crate) fn marker(&mut self, tick: Tick, name: String) {
        self.plan.markers.push(Marker { tick, name });
        self.plan.marker_tags.push(None);
    }

    /// The sequence's name.
    pub(crate) fn name(&mut self, name: String) {
        self.plan.name = name;
    }

    /// The song info from tagged comments (Ruling 9): a tag named as a
    /// DAWproject `MetaData` field (any case) sets that field; the rest are
    /// appended to `Comment` as `Name: value` lines. Each field is cut to
    /// [`MAX_INFO_BYTES`] ([`INFO_CUT`], by field).
    pub(crate) fn info_from_tags(&mut self, tags: &[(String, String)], report: &mut Report) {
        let mut info = SongInfo::default();
        let comment_at = METADATA_FIELDS.len().saturating_sub(1);
        let mut extra: Vec<String> = Vec::new();
        {
            let mut fields = info.fields_mut();
            for (name, value) in tags {
                let known = METADATA_FIELDS
                    .iter()
                    .position(|f| f.eq_ignore_ascii_case(name.trim()));
                match known.and_then(|i| fields.get_mut(i)) {
                    Some(out) if out.is_none() => **out = Some(value.clone()),
                    // A second tag of one field joins the comment.
                    _ => extra.push(format!("{name}: {value}")),
                }
            }
            if !extra.is_empty()
                && let Some(c) = fields.get_mut(comment_at)
            {
                let mut text = c.take().unwrap_or_default();
                for line in &extra {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(line);
                }
                **c = Some(text);
            }
        }
        for (i, out) in info.fields_mut().into_iter().enumerate() {
            let Some(text) = out.take() else { continue };
            let (kept, cut) = cut_to(text.trim_end(), MAX_INFO_BYTES);
            if cut {
                let field = METADATA_FIELDS.get(i).copied().unwrap_or("");
                report.add(ItemKind::Changed, INFO_CUT, Some(field.to_owned()));
            }
            if !kept.trim().is_empty() {
                *out = Some(kept.to_owned());
            }
        }
        self.plan.info = info;
    }

    /// The import: each track's items in time order, with `report`.
    pub(crate) fn build(mut self, report: Report) -> DpImport {
        for v in &mut self.items {
            v.sort_by_key(|i| i.tick);
        }
        self.plan.markers.sort_by_key(|m| m.tick);
        let n = self.items.len();
        self.plan.arranged = Arranged {
            items: self.items,
            seqs: Vec::new(),
            media: self.book.media,
            main: vec![0; n],
        };
        DpImport {
            report,
            plan: self.plan,
            trusted: false,
        }
    }
}

/// The plan's tracks, in order.
pub(crate) fn track_count(imp: &DpImport) -> usize {
    imp.plan.tracks.len()
}

/// The plan's sequence name.
pub(crate) fn plan_name(imp: &DpImport) -> &str {
    &imp.plan.name
}

/// The plan's song info.
pub(crate) fn plan_info(imp: &DpImport) -> &SongInfo {
    &imp.plan.info
}

/// The plan's markers, in tick order.
pub(crate) fn plan_markers(imp: &DpImport) -> &[Marker] {
    &imp.plan.markers
}

/// The plan's tempo and meter maps.
pub(crate) fn plan_maps(imp: &DpImport) -> (&TempoMap, &MeterMap) {
    (&imp.plan.tempo, &imp.plan.meter)
}
