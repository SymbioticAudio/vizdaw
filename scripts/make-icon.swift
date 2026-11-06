// Renders one SymbioticAudio glyph (all rights reserved; read from the
// symbiotic-design checkout, never copied into this repo) as the app icon:
// black on a gold rounded tile, like the website's favicon. Usage: make-icon <glyph.svg> <out.iconset>
import AppKit
let args = CommandLine.arguments
var svg = try! String(contentsOfFile: args[1], encoding: .utf8)
svg = svg.replacingOccurrences(of: "currentColor", with: "#000000")
    .replacingOccurrences(of: "stroke-width=\"1.2\"", with: "stroke-width=\"1.6\"")
guard let glyph = NSImage(data: svg.data(using: .utf8)!) else { fatalError("svg") }
let out = URL(fileURLWithPath: args[2])
try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
func render(_ px: Int) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let s = CGFloat(px) / 1024
    let tile = NSRect(x: 100 * s, y: 100 * s, width: 824 * s, height: 824 * s)
    let path = NSBezierPath(roundedRect: tile, xRadius: 185 * s, yRadius: 185 * s)
    NSGradient(starting: NSColor(srgbRed: 0.957, green: 0.784, blue: 0.267, alpha: 1),
               ending: NSColor(srgbRed: 0.918, green: 0.722, blue: 0.157, alpha: 1))!.draw(in: path, angle: -90)
    // The glyph spans ~20/24 of its box; scale it to fill ~68% of the tile.
    let g = 824 * s * 0.82
    glyph.draw(in: NSRect(x: 512 * s - g / 2, y: 512 * s - g / 2, width: g, height: g))
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}
for (pt, name) in [(16, "16x16"), (32, "32x32"), (128, "128x128"), (256, "256x256"), (512, "512x512")] {
    try! render(pt).write(to: out.appendingPathComponent("icon_\(name).png"))
    try! render(pt * 2).write(to: out.appendingPathComponent("icon_\(name)@2x.png"))
}
