import Foundation
import Security
import VizDAWAppModel

/// Standing approvals in the login Keychain (plan 8b Ruling 11): one
/// generic-password item, service "VizDAW Co-Producer approvals", account
/// "approvals", holding the approvals as JSON. Not preferences or a file,
/// which one `defaults` or `echo` command could write; the Keychain asks
/// the composer before another program reads or changes VizDAW's item.
/// Tests never use it (`MemoryApprovalStore`), nor does the e2e
/// (`VIZDAW_TEST_HOOKS=1`).
struct KeychainApprovalStore: CoProducerApprovalStore {
    static let service = "VizDAW Co-Producer approvals"
    static let account = "approvals"

    private var item: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.service,
            kSecAttrAccount as String: Self.account,
        ]
    }

    /// Every approval; an item that is missing or cannot be read is none,
    /// so VizDAW asks.
    func load() -> [StandingApproval] {
        var query = item
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess, let data = result as? Data else {
            return []
        }
        return StandingApproval.decodeAll(data)
    }

    func save(_ all: [StandingApproval]) throws {
        let data = try StandingApproval.encodeAll(all)
        let update = [kSecValueData as String: data] as CFDictionary
        var status = SecItemUpdate(item as CFDictionary, update)
        if status == errSecItemNotFound {
            var add = item
            add[kSecValueData as String] = data
            add[kSecAttrLabel as String] = Self.service
            status = SecItemAdd(add as CFDictionary, nil)
        }
        guard status == errSecSuccess else {
            throw NSError(
                domain: NSOSStatusErrorDomain, code: Int(status),
                userInfo: [NSLocalizedDescriptionKey: "The Co-Producer approvals could not be saved (\(status))."])
        }
    }
}
