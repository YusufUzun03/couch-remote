import SwiftUI
import WebKit
import CryptoKit

/// Hosts the CouchRemote web app. SPIKE: the TLS challenge handler trusts the certificate presented by the
/// single host the user typed in, and logs its SHA-256. The real version pins the CouchRemote CA instead.
struct WebView: UIViewRepresentable {
    let url: URL
    let log: DiagLog

    func makeCoordinator() -> Coordinator { Coordinator(host: url.host ?? "", log: log) }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.allowsInlineMediaPlayback = true
        let controller = WKUserContentController()
        controller.add(context.coordinator, name: "diag")
        controller.addUserScript(WKUserScript(source: Coordinator.bridgeScript, injectionTime: .atDocumentStart, forMainFrameOnly: false))
        config.userContentController = controller

        let view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = context.coordinator
        view.isOpaque = false
        view.scrollView.contentInsetAdjustmentBehavior = .never
        log.webView = view
        log.add("load \(url.absoluteString)")
        view.load(URLRequest(url: url))
        return view
    }

    func updateUIView(_ view: WKWebView, context: Context) {}

    final class Coordinator: NSObject, WKNavigationDelegate, WKScriptMessageHandler {
        let host: String
        let log: DiagLog
        init(host: String, log: DiagLog) { self.host = host; self.log = log }

        /// Forwards console output, script errors and WebSocket lifecycle events to the native log.
        static let bridgeScript = """
        (function () {
          function send(kind, msg) { try { window.webkit.messageHandlers.diag.postMessage(kind + ': ' + msg); } catch (e) {} }
          ['log', 'warn', 'error'].forEach(function (level) {
            var orig = console[level];
            console[level] = function () {
              send('console.' + level, Array.prototype.slice.call(arguments).join(' '));
              orig.apply(console, arguments);
            };
          });
          window.addEventListener('error', function (e) { send('error', e.message); });
          window.addEventListener('unhandledrejection', function (e) { send('rejection', String(e.reason)); });
          var Native = window.WebSocket;
          function Wrapped(url, protocols) {
            var ws = protocols === undefined ? new Native(url) : new Native(url, protocols);
            ws.addEventListener('open', function () { send('ws', 'open ' + url); });
            ws.addEventListener('close', function (e) { send('ws', 'close ' + e.code + ' ' + url); });
            ws.addEventListener('error', function () { send('ws', 'error ' + url); });
            return ws;
          }
          Wrapped.prototype = Native.prototype;
          Wrapped.CONNECTING = 0; Wrapped.OPEN = 1; Wrapped.CLOSING = 2; Wrapped.CLOSED = 3;
          window.WebSocket = Wrapped;
        })();
        """

        func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
            log.add("\(message.body)")
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            log.add("page loaded: \(webView.url?.absoluteString ?? "?")")
        }

        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            log.add("navigation failed: \(error.localizedDescription)")
        }

        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
            log.add("load failed: \(error.localizedDescription)")
        }

        func webView(_ webView: WKWebView, didReceive challenge: URLAuthenticationChallenge,
                     completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
            let space = challenge.protectionSpace
            guard space.authenticationMethod == NSURLAuthenticationMethodServerTrust,
                  let trust = space.serverTrust else {
                completionHandler(.performDefaultHandling, nil)
                return
            }
            guard space.host == host else {
                log.add("tls: other host \(space.host), default handling")
                completionHandler(.performDefaultHandling, nil)
                return
            }
            var note = "tls: challenge for \(space.host):\(space.port)"
            if let chain = SecTrustCopyCertificateChain(trust) as? [SecCertificate], let leaf = chain.first {
                let digest = SHA256.hash(data: SecCertificateCopyData(leaf) as Data)
                note += " leaf sha256=" + digest.map { String(format: "%02x", $0) }.joined().prefix(16) + "... chain=\(chain.count)"
            }
            log.add(note + " -> trusting (spike)")
            completionHandler(.useCredential, URLCredential(trust: trust))
        }
    }
}
