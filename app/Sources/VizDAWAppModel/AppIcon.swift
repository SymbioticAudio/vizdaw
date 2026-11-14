import AppKit
import SwiftUI
import SymbioticIcons
import SymbioticTokens
import VizDAWCore

/// Which SymbioticAudio set an AppIcon case draws from (final fix wave B).
public enum IconSource: Sendable, Hashable {
    /// The default set: the registered brand glyph where it maps the role,
    /// else the standard icon (a `VIZDAW_NO_BRAND=1` build).
    case glyph
    /// The angular set (symbiotic-design 0.2.2), brand or not.
    case angular

    /// The SymbioticIcons set this source draws.
    public var set: IconSet {
        switch self {
        case .glyph: .standard
        case .angular: .angular
        }
    }
}

/// The icons VizDAW shows, by meaning: the one place an icon is chosen
/// (plan 7b Task B3). Every case draws a SymbioticAudio icon, its `role`
/// from its `source` (owner, 2026-11-04); none draws an SF Symbol.
public enum AppIcon: CaseIterable, Sendable {
    // Status.
    case warning, problem, fault, info
    /// The transport bar's Faults button (a dot, tinted by severity).
    case faultIndicator
    case mixer
    // Editing.
    case add, remove, confirm, close
    case zoomIn, zoomOut
    // Inputs and editors.
    case audio, drumGrid, pianoRoll, list
    case muted, loop, split, empty
    case record, armed, pencil, diskSlow
    /// The Record button while recording.
    case recording
    case expand, collapse, menuDown
    // The pane toggles.
    case leftPane, bottomPane, rightPane
    // The Settings tabs.
    case settingsEditing, settingsKeys, settingsAudio, settingsRecording, settingsAutomation, settingsSurfaces,
        settingsAppearance, settingsPlugins
    case eq
    // Beyond the plan's list (the transport bar and the learn banner).
    case play, stop, midiLearn
    // The transport bar's jumps.
    case returnToStart, rewind, fastForward
    // The track-kind icons (plan tracks Ruling 6): symbiotic-design 0.2.2 has
    // no track-kind roles, so each borrows one.
    case audioTrack, instrumentTrack, midiTrack

    /// The icon of a track of `kind`.
    public static func track(_ kind: TrackKindRow) -> AppIcon {
        switch kind {
        case .audio: .audioTrack
        case .instrument: .instrumentTrack
        case .midi: .midiTrack
        }
    }

    public var role: IconRole {
        switch self {
        case .warning: .statusWarning
        case .problem: .statusProblem
        case .fault: .statusFault
        case .faultIndicator: .statusFaultIndicator
        case .info: .statusInfo
        case .mixer: .viewMixer
        case .add: .editAdd
        case .remove: .editRemove
        case .confirm: .editConfirm
        case .close: .editClose
        case .zoomIn: .viewZoomIn
        case .zoomOut: .viewZoomOut
        case .audio: .inputAudio
        case .drumGrid: .editorDrumGrid
        case .pianoRoll: .editorPianoRoll
        case .list: .editorList
        case .muted: .statusAudioMuted
        case .loop: .modeLoop
        case .split: .viewSplit
        case .empty: .viewEmpty
        case .record: .transportRecord
        case .recording: .transportRecording
        case .armed: .recordArm
        case .pencil: .toolPencil
        case .diskSlow: .statusDiskSlow
        case .expand: .viewExpand
        case .collapse: .viewCollapse
        case .menuDown: .navDown
        case .leftPane: .viewLeftPane
        case .bottomPane: .viewBottomPane
        case .rightPane: .viewRightPane
        case .settingsEditing: .toolPencil
        case .settingsKeys: .settingsKeys
        case .settingsAudio: .settingsAudio
        case .settingsRecording: .transportRecord
        case .settingsAutomation: .editorEnvelope
        case .settingsSurfaces: .settingsSurfaces
        case .settingsAppearance: .settingsAppearance
        case .settingsPlugins: .mixerEq
        case .eq: .mixerEq
        case .play: .transportPlay
        case .stop: .transportStop
        case .midiLearn: .controlMidiLearn
        case .returnToStart: .transportReturnToZero
        case .rewind: .transportRewind
        case .fastForward: .transportFastForward
        case .audioTrack: .inputAudio
        case .instrumentTrack: .slotSource
        case .midiTrack: .controlMidiLearn
        }
    }

    /// The set the case draws from (owner, 2026-11-04): the owner's 26
    /// glyph picks and the 18 angular icons. Switching one icon back is a
    /// one-line change here. settingsAppearance, in neither list, draws the
    /// default set.
    public var source: IconSource {
        switch self {
        case .fault, .faultIndicator, .remove, .zoomIn, .zoomOut, .audio, .pianoRoll, .record, .armed, .recording,
            .settingsKeys, .settingsAudio, .settingsRecording, .play, .stop, .returnToStart, .rewind, .fastForward,
            .audioTrack:
            .angular
        case .warning, .problem, .info, .mixer, .add, .confirm, .close, .drumGrid, .list, .muted, .loop, .split,
            .empty, .pencil, .diskSlow, .expand, .collapse, .menuDown, .leftPane, .bottomPane, .rightPane,
            .settingsEditing, .settingsAutomation, .settingsSurfaces, .settingsAppearance, .settingsPlugins, .eq, .midiLearn,
            .instrumentTrack, .midiTrack:
            .glyph
        }
    }

    /// The SF Symbol the app showed before 7b, kept as data for reverting a
    /// case. Unused while every case has a Symbiotic source: nothing draws
    /// it. pianoRoll, armed, pencil, settingsAppearance and eq had no icon
    /// then; theirs are placeholders.
    public var symbol: String {
        switch self {
        case .warning: "exclamationmark.triangle"
        case .problem: "exclamationmark.circle"
        case .fault: "xmark.octagon"
        case .info: "info.circle"
        case .faultIndicator: "circle.fill"
        case .mixer: "slider.vertical.3"
        case .add: "plus"
        case .remove: "minus.circle"
        case .confirm: "checkmark"
        case .close: "xmark"
        case .zoomIn: "plus.magnifyingglass"
        case .zoomOut: "minus.magnifyingglass"
        case .audio: "waveform"
        case .drumGrid: "square.grid.3x3"
        case .pianoRoll: "pianokeys"
        case .list: "music.note.list"
        case .muted: "speaker.slash"
        case .loop: "repeat"
        case .split: "rectangle.split.1x2"
        case .empty: "xmark.rectangle"
        case .record: "record.circle"
        case .armed: "record.circle"
        case .pencil: "pencil"
        case .diskSlow: "externaldrive.badge.exclamationmark"
        case .recording: "record.circle.fill"
        case .expand: "arrow.up.left.and.arrow.down.right"
        case .collapse: "arrow.down.right.and.arrow.up.left"
        case .menuDown: "chevron.down"
        case .leftPane: "sidebar.left"
        case .bottomPane: "rectangle.bottomthird.inset.filled"
        case .rightPane: "sidebar.right"
        case .settingsEditing: "pencil"
        case .settingsKeys: "keyboard"
        case .settingsAudio: "hifispeaker"
        case .settingsRecording: "record.circle"
        case .settingsAutomation: "slider.vertical.3"
        case .settingsSurfaces: "dial.medium"
        case .settingsAppearance: "paintpalette"
        case .settingsPlugins: "slider.horizontal.3"
        case .eq: "slider.horizontal.3"
        case .play: "play.fill"
        case .stop: "stop.fill"
        case .midiLearn: "pianokeys"
        case .returnToStart: "backward.end.fill"
        case .rewind: "backward.fill"
        case .fastForward: "forward.fill"
        case .audioTrack: "waveform"
        case .instrumentTrack: "pianokeys"
        case .midiTrack: "pianokeys.inverse"
        }
    }

    /// A template image of the case at `size` points in `theme`, drawn by
    /// SymbioticIcons from its `source` (`resolver`: tests and previews).
    @MainActor
    public func nsImage(
        accessibilityDescription: String?, size: CGFloat = 16, theme: Theme = ThemedViews.current,
        resolver: IconResolver = Icons.resolver
    ) -> NSImage {
        let image = NSImage.icon(role, size: size, theme: theme, set: source.set, resolver: resolver)
        image.isTemplate = true
        image.accessibilityDescription = accessibilityDescription
        return image
    }

    /// The case as a SwiftUI template image at `size` points, labelled with
    /// its role's name, in the current theme.
    @MainActor
    public func image(size: CGFloat) -> Image {
        Image(nsImage: nsImage(accessibilityDescription: role.label, size: size)).renderingMode(.template)
    }

    /// The case as a 16-point SwiftUI template image.
    @MainActor public var image: Image { image(size: 16) }

    /// `Label(title)` with the case's 16-point image (or `size`).
    @MainActor
    public func label(_ title: String, size: CGFloat = 16) -> Label<Text, Image> {
        Label { Text(title) } icon: { image(size: size) }
    }
}
