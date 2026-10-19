import VizDAWCore

/// What Modify Again repeats and the saved templates, at one snapshot
/// version.
struct ModifyReads {
    let version: UInt64
    let last: ModifyRow?
    let templates: [ModifyTemplateRow]
}

/// Templates and Modify Again (plan 4c), read once per snapshot version for
/// the menus; a failed read (Busy) keeps the last values, silently.
extension EditorModel {
    /// The modification Modify Again repeats, if any.
    public func lastModify() -> ModifyRow? { modifyReads().last }

    /// The saved select-and-modify templates, by name.
    public func modifyTemplates() -> [ModifyTemplateRow] { modifyReads().templates }

    func modifyReads() -> ModifyReads {
        if let c = modifyCache, c.version == snapshotVersion { return c }
        do {
            let c = ModifyReads(
                version: snapshotVersion, last: try session.lastModify(), templates: try session.modifyTemplates())
            modifyCache = c
            return c
        } catch {
            return modifyCache ?? ModifyReads(version: snapshotVersion, last: nil, templates: [])
        }
    }
}
