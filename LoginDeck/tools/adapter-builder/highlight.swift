import AppKit

// A transient, nonactivating outline. No event interception or screen capture.
let values = CommandLine.arguments.dropFirst().compactMap(Double.init)
guard values.count == 4, values.allSatisfy({ $0.isFinite }), values[2] > 0, values[3] > 0 else { exit(2) }
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
guard let main = NSScreen.screens.first else { exit(3) }
let rect = NSRect(x: values[0], y: main.frame.maxY - values[1] - values[3], width: values[2], height: values[3])
let panel = NSPanel(contentRect: rect.insetBy(dx: -3, dy: -3), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
panel.isOpaque = false
panel.backgroundColor = .clear
panel.hasShadow = false
panel.ignoresMouseEvents = true
panel.level = .floating
panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
let view = NSView(frame: NSRect(origin: .zero, size: panel.frame.size))
view.wantsLayer = true
view.layer?.borderColor = NSColor.systemRed.cgColor
view.layer?.borderWidth = 3
view.layer?.cornerRadius = 5
panel.contentView = view
panel.orderFrontRegardless()
DispatchQueue.main.asyncAfter(deadline: .now() + 2) { app.terminate(nil) }
app.run()
panel.orderOut(nil)
