import VizDAWCore

/// A drag in progress: one undo step named `label` (spec §3.2, §3.8).
public struct Gesture: Equatable, Sendable {
    public let label: String
    /// The lanes it edits; it is cancelled when one disappears (spec §5).
    public internal(set) var lanes: Set<LaneKey>
    /// The target the document shows now (`nil`: the start state).
    public var applied: EditCommand?
    /// The mouse-up target, until it has been applied and the edit closed.
    public var pendingFinal: EditCommand?
    /// Mouse-up came; `tick()` retries what is left.
    public var finishing = false
    /// An intermediate edit met Busy, so `applied` was forgotten: the next
    /// target is always sent, even one equal to the forgotten one, and the
    /// document may no longer be at the start state.
    var uncertain = false
}

extension EditorModel {
    /// Opens a gesture once the pointer left the dead zone. False when
    /// one is already open or the session refused.
    @discardableResult
    public func beginGesture(_ label: String, lanes: Set<LaneKey>) -> Bool {
        guard gesture == nil else { return false }
        do {
            try session.beginEdit(label: label)
            gesture = Gesture(label: label, lanes: lanes)
            return true
        } catch {
            report(error)
            return false
        }
    }

    /// The lanes the open gesture touches now (a drag that changed its
    /// target lane); `afterSync` aborts it when one of them disappears.
    public func setGestureLanes(_ lanes: Set<LaneKey>) {
        guard var g = gesture, g.lanes != lanes else { return }
        g.lanes = lanes
        gesture = g
    }

    /// The pointer moved; `target` is the full change from the drag's start
    /// (`nil`: none). Busy is dropped (the next move resends, even the same
    /// target); a refused target leaves the start state on screen, silently.
    public func updateGesture(_ target: EditCommand?) {
        guard var g = gesture, !g.finishing, target != g.applied || g.uncertain else { return }
        guard let target else {
            rewind(&g)
            if gesture != nil { gesture = g }
            return
        }
        do {
            applySnapshot(try session.editGesture(cmd: target, filter: nil))
            g.applied = target
            g.uncertain = false
        } catch VizError.Busy {
            // Dropped: forget what was applied so the next move, or the
            // mouse-up, sends its full target again.
            g.applied = nil
            g.uncertain = true
        } catch {
            // Refused: a conversion refusal keeps the previous target, a core
            // refusal rewinds to the start; either way `applied` is unknown.
            g.applied = nil
            g.uncertain = true
            if let s = try? session.snapshot() { applySnapshot(s) }
        }
        // applySnapshot may have cancelled the gesture (its lane vanished).
        if gesture != nil { gesture = g }
    }

    /// Mouse-up: `final` becomes the gesture's content and the edit closes
    /// as one undo step; Busy leaves it pending for `tick()`.
    public func endGesture(_ final: EditCommand?) {
        guard var g = gesture else { return }
        g.finishing = true
        g.pendingFinal = final
        gesture = g
        finish()
    }

    /// Escape, or the gesture's lanes vanished: the start state comes back
    /// and the edit closes without an undo step.
    public func cancelGesture() {
        guard gesture != nil else { return }
        gesture = nil
        do {
            applySnapshot(try session.abortEdit())
        } catch {
            report(error)
        }
        refreshSelection()
    }

    /// Retries a mouse-up that met Busy and audition note-offs that failed
    /// (the 60 Hz plugin timer calls it).
    public func tick() {
        if hasPendingNoteOffs { retryNoteOffs() }
        if gesture?.finishing == true { finish() }
    }

    private func rewind(_ g: inout Gesture) {
        guard g.applied != nil || g.uncertain else { return }
        do {
            applySnapshot(try session.abortEdit())
            // applySnapshot may have cancelled the gesture (its lane vanished):
            // reopening the edit then would leave one open with no gesture.
            guard gesture != nil else { return }
            try session.beginEdit(label: g.label)
            g.applied = nil
            g.uncertain = false
        } catch {
            report(error)
        }
    }

    private func finish() {
        guard var g = gesture else { return }
        if g.pendingFinal == nil, g.applied != nil || g.uncertain {
            cancelGesture()
            return
        }
        if let cmd = g.pendingFinal, cmd != g.applied || g.uncertain {
            do {
                applySnapshot(try session.editGesture(cmd: cmd, filter: nil))
                g.applied = cmd
                g.uncertain = false
                guard gesture != nil else { return }
                gesture = g
            } catch VizError.Busy {
                return
            } catch {
                cancelGesture()
                report(error)
                return
            }
        }
        do {
            let s = try session.endEdit()
            gesture = nil
            applySnapshot(s)
            refreshSelection()
        } catch VizError.Busy {
            return
        } catch {
            gesture = nil
            report(error)
            if let s = try? session.abortEdit() { applySnapshot(s) }
        }
    }
}
