import Foundation
import VizDAWCore

/// An error shown to the user.
public struct AlertItem: Identifiable, Equatable, Sendable {
    public let id = UUID()
    public let title: String
    public let message: String

    public init(title: String, message: String) {
        self.title = title
        self.message = message
    }

    /// An alert for any thrown error; `VizError`s show their message.
    public init(_ error: any Error, title: String) {
        self.title = title
        if let e = error as? VizError {
            message = e.message
        } else {
            message = error.localizedDescription
        }
    }

    public static func == (a: AlertItem, b: AlertItem) -> Bool {
        a.title == b.title && a.message == b.message
    }
}

extension VizError {
    /// The human-readable text of the error.
    public var message: String {
        switch self {
        case let .NotFound(path): "No VizDAW project at \(path)."
        case let .Model(message): message
        case let .Io(message): message
        case let .Engine(message): message
        case let .NoDevice(message): "No audio output: \(message)"
        case let .Busy(message): message
        case let .Internal(message): "Internal error: \(message)"
        }
    }
}

extension LaneRow: Identifiable {}
extension SeqRow: Identifiable {}
