import AppKit

struct EditorMinimapRaster {
    private static let glyphs = minimapGlyphs()
    private let lines: [Substring]
    private let lineOffsets: [Int]
    private let kinds: [UInt8]

    var lineCount: Int { lines.count }
    var contentHeight: Int { lineCount * 2 }

    static func build(text: String, fileName: String) -> Self {
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
        var kinds = [UInt8](repeating: 0, count: text.utf16.count)
        for token in highlightDocument(fileName: fileName, text: text) {
            for position in Int(token.startUtf16)..<min(kinds.count, Int(token.endUtf16)) {
                kinds[position] = token.kind
            }
        }
        var lineOffsets = [Int]()
        lineOffsets.reserveCapacity(lines.count)
        var nextOffset = 0
        for line in lines {
            lineOffsets.append(nextOffset)
            nextOffset += line.utf16.count + 1
        }
        return Self(lines: lines, lineOffsets: lineOffsets, kinds: kinds)
    }

    func image(width: Int, height: Int, offset: Int) -> NSImage? {
        let width = max(1, width)
        let height = max(1, height)
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        let firstLine = max(0, offset / 2)
        let lastLine = min(lineCount, (offset + height + 1) / 2)
        if firstLine < lastLine {
            for lineNumber in firstLine..<lastLine {
                let y = lineNumber * 2 - offset
                let lineOffset = lineOffsets[lineNumber]
                var x = 4
                for (column, unit) in lines[lineNumber].utf16.enumerated() {
                    if x >= width - 4 { break }
                    if unit == 9 {
                        x += 4 - (x - 4) % 4
                        continue
                    }
                    if unit != 32 && unit != 13 {
                        let kind = lineOffset + column < kinds.count ? kinds[lineOffset + column] : 0
                        let rgb: (UInt8, UInt8, UInt8) = switch kind {
                        case 1: (170, 112, 211)
                        case 2: (216, 139, 68)
                        case 3: (94, 181, 105)
                        case 4: (133, 142, 151)
                        case 5: (86, 154, 224)
                        default: (155, 164, 174)
                        }
                        let glyphIndex = (unit >= 32 && unit <= 126 ? Int(unit - 32) : 95) * 2
                        for dy in 0..<2 where y + dy >= 0 && y + dy < height {
                            let coverage = Self.glyphs[glyphIndex + dy]
                            if coverage == 0 { continue }
                            let pixel = ((y + dy) * width + x) * 4
                            pixels[pixel] = rgb.0
                            pixels[pixel + 1] = rgb.1
                            pixels[pixel + 2] = rgb.2
                            pixels[pixel + 3] = UInt8(min(255, Int(coverage) * 2))
                        }
                    }
                    x += 1
                }
            }
        }
        guard let bitmap = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
            isPlanar: false, colorSpaceName: .deviceRGB,
            bytesPerRow: width * 4, bitsPerPixel: 32
        ), let destination = bitmap.bitmapData else { return nil }
        pixels.withUnsafeBytes { bytes in
            if let source = bytes.baseAddress { memcpy(destination, source, bytes.count) }
        }
        let image = NSImage(size: NSSize(width: width, height: height))
        image.addRepresentation(bitmap)
        return image
    }
}
