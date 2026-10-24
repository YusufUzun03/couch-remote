import SwiftUI
import AVFoundation

struct RootView: View {
    @AppStorage("serverURL") private var serverURL = ""
    @StateObject private var log = DiagLog()
    @State private var pendingCode = ""
    @State private var manualName = ""
    @State private var showLog = false
    @State private var scanning = false
    @State private var working = false
    @State private var message = ""
    @State private var cameraDenied = false

    private var paired: (url: URL, ca: SecCertificate)? {
        guard let url = URL(string: serverURL), url.scheme == "https", url.host != nil, let ca = CAStore.load() else { return nil }
        return (url, ca)
    }

    var body: some View {
        if let paired {
            connectedView(paired)
        } else {
            setupView
        }
    }

    private func connectedView(_ paired: (url: URL, ca: SecCertificate)) -> some View {
        // The one-time pairing code rides along on the first load only; the page removes it from the address itself.
        var target = paired.url
        if !pendingCode.isEmpty, var parts = URLComponents(url: paired.url, resolvingAgainstBaseURL: false) {
            parts.queryItems = [URLQueryItem(name: "c", value: pendingCode)]
            target = parts.url ?? paired.url
        }
        return ZStack(alignment: .topTrailing) {
            WebView(url: target, ca: paired.ca, log: log, onLoaded: { pendingCode = "" })
                .ignoresSafeArea()
            Button {
                showLog = true
            } label: {
                Image(systemName: "ladybug")
                    .font(.system(size: 15, weight: .semibold))
                    .padding(10)
                    .background(.ultraThinMaterial, in: Circle())
            }
            .padding(.trailing, 12)
            .padding(.top, 4)
        }
        .sheet(isPresented: $showLog) {
            DiagSheet(log: log, onChangeServer: {
                showLog = false
                serverURL = ""
                CAStore.clear()
            })
        }
    }

    private var setupView: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Connect to your PC")
                .font(.title2.weight(.semibold))
            Text("Open CouchRemote on your PC, then scan the QR code shown on its page.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
            Button {
                startScan()
            } label: {
                Label("Scan QR code", systemImage: "qrcode.viewfinder")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(working)
            if cameraDenied {
                Text("Camera access is off. Enable it in Settings › CouchRemote, or enter the PC name below.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            DisclosureGroup("Can't scan? Enter the PC name") {
                VStack(alignment: .leading, spacing: 10) {
                    TextField("couch-xxxx.local", text: $manualName)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                        .padding(12)
                        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 10))
                    Button("Connect") {
                        if let link = SetupLink(manual: manualName) { pair(link) } else { message = "That does not look like a PC name." }
                    }
                    .buttonStyle(.bordered)
                    .disabled(working || manualName.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                .padding(.top, 8)
            }
            DisclosureGroup("Don't have the PC program yet?") {
                VStack(alignment: .leading, spacing: 10) {
                    Text("CouchRemote needs a small free program on your Windows PC. Open this page on the PC to download it:")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                    Text(Links.site.absoluteString)
                        .font(.footnote.monospaced())
                        .textSelection(.enabled)
                    HStack {
                        ShareLink(item: Links.site) { Label("Send link to PC", systemImage: "square.and.arrow.up") }
                        Spacer()
                        Link("Open page", destination: Links.site)
                    }
                    .buttonStyle(.bordered)
                }
                .padding(.top, 8)
            }
            if working { ProgressView("Connecting…") }
            if !message.isEmpty {
                Text(message).font(.footnote).foregroundStyle(.red)
            }
            Spacer()
        }
        .padding(20)
        .fullScreenCover(isPresented: $scanning) {
            ZStack(alignment: .topTrailing) {
                QRScanner { code in
                    scanning = false
                    if let link = SetupLink(qr: code) { pair(link) } else { message = "That QR code is not from CouchRemote." }
                }
                .ignoresSafeArea()
                Button("Cancel") { scanning = false }
                    .padding(12)
                    .background(.ultraThinMaterial, in: Capsule())
                    .padding(16)
            }
        }
    }

    private func startScan() {
        message = ""
        AVCaptureDevice.requestAccess(for: .video) { granted in
            DispatchQueue.main.async {
                cameraDenied = !granted
                scanning = granted
            }
        }
    }

    /// Fetches the PC's root certificate, stores it, and opens the app (with the one-time code when the QR had one).
    private func pair(_ link: SetupLink) {
        working = true
        message = ""
        Task {
            do {
                let der = try await CAStore.fetch(from: link)
                await MainActor.run {
                    CAStore.save(der)
                    pendingCode = link.code ?? ""
                    serverURL = link.appURL.absoluteString
                    working = false
                }
            } catch {
                await MainActor.run {
                    message = error.localizedDescription
                    working = false
                }
            }
        }
    }
}

struct DiagSheet: View {
    @ObservedObject var log: DiagLog
    let onChangeServer: () -> Void

    var body: some View {
        NavigationStack {
            ScrollView {
                Text(log.lines.isEmpty ? "No events yet." : log.lines.joined(separator: "\n"))
                    .font(.system(size: 11, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
                    .textSelection(.enabled)
            }
            .navigationTitle("Diagnostics")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItemGroup(placement: .bottomBar) {
                    Button("Run checks") { log.runChecks() }
                    Spacer()
                    Button("Clear") { log.clear() }
                    Spacer()
                    Button("Change server", role: .destructive, action: onChangeServer)
                }
            }
        }
        .presentationDetents([.medium, .large])
    }
}
