# AAF test fixtures

No AAF file is committed here. Every AAF the tests read is built while the
test runs, in a temp folder, by the builders in `tests/common/aaf.rs`, so
nothing third-party is committed and every fixture can be read in the code
that makes it.

## How each fixture is built

- **Object stores** (`tests/aaf_store.rs`): `tree()` builds a small store
  through `Store::add_child`, and `AafWriter` writes it. The hostile cases use
  `raw_file()` and `props()`, which write any compound-file storage or
  `properties` bytes directly through `cfb`, so they can break rules the
  writer keeps.
- **Compositions** (`tests/aaf_read.rs`): `Fx` builds a file of mobs.
  - `Fx::file(&FileSpec)` adds one audio file: a file `SourceMob` with its
    descriptor (`WAVEDescriptor`, `AIFCDescriptor` or `PCMDescriptor`), an
    `EssenceData` holding the bytes when embedded or a `NetworkLocator` when
    external, and the `MasterMob` that plays it. Both mobs have one slot per
    channel. `wav_bytes()` makes silent 16-bit WAV files.
  - `Fx::comp(name, top_level, slots)` adds a `CompositionMob`. Its slots are
    `SlotSpec`s: a sound slot holds a `Seg` tree (`Fill`, `Clip`, `Trans`,
    `Seq`, `Op`, `Select`), and a slot can also be another data kind, a
    timecode or an event slot of markers.
  - `Fx::mob` adds a mob of any class with given slots; the loop case uses it
    to make two master mobs that play each other.
  - `Fx::product` adds an `Identification`. Fixtures name their application
    "Fixture", or "VizDAW" to test files from VizDAW.

  The layout follows plan 6f's Rulings 1 and 4 and the AAF Object
  Specification. Locators name only paths inside the test's temp folder.
- **Exports** (`tests/aaf_export.rs`, `tests/aaf_round_trip.rs`): documents
  built with `core_engine::fixtures::Builder` and 6d's generators are
  exported with `core_io::aaf::export`, and `write_export()` writes each into
  a temp folder, streaming a silent WAV (`wav_pcm()`, 16 or 24-bit) for every
  staged file. The round trips read the file back with `AafFile::open`, map
  it with `Online` items standing for the staged files, and compare
  `projection()`s.

The definition and class IDs come from `core_io::aaf::dict`, which was
checked against the model tables of pyaaf2
(<https://github.com/markreidvfx/pyaaf2>, MIT). LibAAF
(<https://github.com/agfline/LibAAF>) was read for how other applications
shape their files. `core_io::aaf::metadict` is generated from pyaaf2's model
tables (Ruling 11) and keeps their MIT notice; no other code or file is taken
from either.

## `src/aaf/metadict.rs`, generated (Ruling 11)

`metadict.rs` holds the MetaDictionary every export writes whole: 115
classes with their properties and 164 types. It is data generated from
pyaaf2's model tables, so it keeps pyaaf2's MIT copyright and licence
notice at its top; no pyaaf2 code is ported or run. It was made once, on
2026-11-13, like this:

1. Fetch, at pyaaf2 commit `08dcc3dfe823ea5781db1cb657d9c2606557fcdc`,
   `src/aaf2/model/classdefs.py`, `src/aaf2/model/typedefs.py`,
   `src/aaf2/metadict.py` and `LICENSE` from
   `https://raw.githubusercontent.com/markreidvfx/pyaaf2/<commit>/...` into a
   temp folder outside the repo.
2. Run a one-off script (not committed) that reads those files as text:
   it parses each top-level `name = {...}` literal with Python's
   `ast.literal_eval`, so nothing from pyaaf2 executes. It writes:
   - the licence header, quoting `LICENSE` whole;
   - the module's hand-written part (the row types, the lookups and
     `write_into`), kept beside the script as a text file and pasted in
     unchanged;
   - `ROOT`: `metadict.py`'s `root_classes` (the root's two properties;
     pyaaf2 does not put `Root` in the file's `ClassDefinitions`);
   - `CLASSES`: `classdefs.py`'s `classdefs`, in dict order. A class with
     no parent names itself as its parent, as the file stores it. A
     property whose PID is `None` gets pyaaf2's dynamic PID: counted down
     from 0xFFFF in table order, skipping explicit PIDs at or above 0x8000
     met so far (only `TypeDefinitionGenericCharacter.CharacterSize`,
     0xFFBC, is ever written);
   - `TYPES`: `metadict.py`'s `root_types` first, then `typedefs.py`'s
     categories in pyaaf2's registration order (ints, enums, records, fixed
     arrays, variable arrays, renames, strings, streams, opaques, extendible
     enums, characters, generic characters, indirects, sets, strong
     references, weak references), each in dict order.
3. `cargo fmt`, then check: every property's type and every class's parent
   is in the tables (asserted by `dict_agrees_with_the_meta_dictionary`),
   every class AUID and PID in `dict.rs` equals its row by name (the same
   test), and ten rows by hand against the AAF Object Specification v1.1
   (for the committed file these ten were checked against `dict.rs` only;
   the check against the Specification itself is still to do):
   `SourceClip` (its AUID, parent `SourceReference`, `StartTime` 0x1201
   optional), `Mob.MobID` (0x4401, required, unique), `Component.Length`
   (0x0202, optional), `Transition.CutPoint` (0x1802, required),
   `TimelineMobSlot.EditRate` (0x4b01) and `Origin` (0x4b02, required),
   `Header.ByteOrder` (0x3b01, Int16), `WAVEDescriptor.Summary` (0x3801,
   required), `EssenceData.Data` (0x2702, a stream), `ControlPoint.Time`
   (0x1a03, a rational) and `Identification.ProductID` (0x3c05, an AUID).

To regenerate after a pinned-commit change, repeat the steps and review the
diff: the tables are data, and the writer reads everything from them.

## Other files

`VIZDAW_INTERCHANGE_SAMPLES=<dir> cargo test -p core-io --test aaf_read
reads_local_samples -- --nocapture` opens every `.aaf` below `<dir>`. Each
must open, or be refused in a sentence; a panic or an I/O error fails the
test. It prints each file's track and clip counts. The test is skipped when
the variable is unset, and refuses a folder inside `~/Library`.
