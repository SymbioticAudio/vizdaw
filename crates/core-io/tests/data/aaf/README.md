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

The definition and class IDs come from `core_io::aaf::dict`, which was
checked against the model tables of pyaaf2
(<https://github.com/markreidvfx/pyaaf2>, MIT). LibAAF
(<https://github.com/agfline/LibAAF>) was read for how other applications
shape their files. No code or file is taken from either.

## Other files

`VIZDAW_INTERCHANGE_SAMPLES=<dir> cargo test -p core-io --test aaf_read
reads_local_samples -- --nocapture` opens every `.aaf` below `<dir>`. Each
must open, or be refused in a sentence; a panic or an I/O error fails the
test. It prints each file's track and clip counts. The test is skipped when
the variable is unset, and refuses a folder inside `~/Library`.
