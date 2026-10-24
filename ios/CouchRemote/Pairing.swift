import Foundation
import Security

/// What the PC's setup QR code carries: `http://<name>.local:47800/setup?c=<one-time code>`.
struct SetupLink {
    let host: String
    let httpPort: Int
    let code: String?

    init?(qr text: String) {
        guard let url = URL(string: text.trimmingCharacters(in: .whitespacesAndNewlines)),
              url.scheme == "http", let host = url.host, url.path == "/setup" else { return nil }
        self.host = host
        self.httpPort = url.port ?? 80
        self.code = URLComponents(url: url, resolvingAgainstBaseURL: false)?
            .queryItems?.first(where: { $0.name == "c" })?.value
    }

    /// A bare PC name such as `couch-63de.local` typed by hand; the page asks for the PIN itself.
    init?(manual text: String) {
        var name = text.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        for prefix in ["https://", "http://"] where name.hasPrefix(prefix) { name.removeFirst(prefix.count) }
        name = String(name.prefix { $0 != ":" && $0 != "/" })
        guard !name.isEmpty, name.allSatisfy({ $0.isLetter || $0.isNumber || $0 == "-" || $0 == "." }) else { return nil }
        self.host = name
        self.httpPort = 47800
        self.code = nil
    }

    /// The app lives one port above the setup server.
    var appURL: URL { URL(string: "https://\(host):\(httpPort + 1)/app")! }
}

enum PairingError: LocalizedError {
    case unreachable(String)
    case badCertificate

    var errorDescription: String? {
        switch self {
        case .unreachable(let detail): return "Could not reach the PC (\(detail)). Is CouchRemote running and is the phone on the same Wi-Fi?"
        case .badCertificate: return "The PC returned something that is not a certificate."
        }
    }
}

/// The CouchRemote root certificate this phone trusts. It is public data, so UserDefaults is enough;
/// only this one CA, and only for the stored PC name, is ever accepted.
enum CAStore {
    private static let key = "caDER"

    static func load() -> SecCertificate? {
        guard let der = UserDefaults.standard.data(forKey: key) else { return nil }
        return SecCertificateCreateWithData(nil, der as CFData)
    }

    static func save(_ der: Data) { UserDefaults.standard.set(der, forKey: key) }
    static func clear() { UserDefaults.standard.removeObject(forKey: key) }

    /// Downloads the PC's root certificate from its setup server (trust on first use: the QR code was read
    /// from the PC's own screen). Returns the DER bytes after checking they parse as a certificate.
    static func fetch(from link: SetupLink) async throws -> Data {
        guard let url = URL(string: "http://\(link.host):\(link.httpPort)/ca.crt") else { throw PairingError.unreachable("bad address") }
        var request = URLRequest(url: url)
        request.timeoutInterval = 8
        do {
            let (data, response) = try await URLSession.shared.data(for: request)
            guard (response as? HTTPURLResponse)?.statusCode == 200,
                  SecCertificateCreateWithData(nil, data as CFData) != nil else { throw PairingError.badCertificate }
            return data
        } catch let error as PairingError {
            throw error
        } catch {
            throw PairingError.unreachable(error.localizedDescription)
        }
    }
}
