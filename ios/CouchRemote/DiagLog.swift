import Foundation
import WebKit

/// Diagnostics: collects what the web view and the TLS challenge handler report.
final class DiagLog: ObservableObject {
    @Published private(set) var lines: [String] = []
    weak var webView: WKWebView?

    private static let stamp: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "HH:mm:ss.SSS"
        return f
    }()

    func add(_ text: String) {
        DispatchQueue.main.async {
            self.lines.append("\(Self.stamp.string(from: Date()))  \(text)")
            if self.lines.count > 400 { self.lines.removeFirst(self.lines.count - 400) }
        }
    }

    func clear() {
        DispatchQueue.main.async { self.lines.removeAll() }
    }

    /// Feature probes that matter for CouchRemote, evaluated inside the page.
    func runChecks() {
        guard let webView else { add("checks: no web view"); return }
        let probes: [(String, String)] = [
            ("location", "location.href"),
            ("secure context", "String(window.isSecureContext)"),
            ("RTCPeerConnection", "typeof RTCPeerConnection"),
            ("serviceWorker", "String('serviceWorker' in navigator)"),
            ("wakeLock", "String('wakeLock' in navigator)"),
            ("vibrate", "String('vibrate' in navigator)"),
            ("user agent", "navigator.userAgent"),
        ]
        for (name, js) in probes {
            webView.evaluateJavaScript(js) { value, error in
                if let error { self.add("check \(name): ERROR \(error.localizedDescription)") }
                else { self.add("check \(name): \(value ?? "nil")") }
            }
        }
    }
}
