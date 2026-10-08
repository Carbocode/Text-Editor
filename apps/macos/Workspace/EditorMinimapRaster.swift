import AppKit

struct EditorMinimapRaster {
    private static let glyphs = minimapGlyphs()
    let width: Int
    let height: Int
    let usedHeight: Int
    let lineCount: Int
    let pixels: [UInt8]

    static func build(text: String, fileName: String, width: Int, height: Int) -> Self {
        let width = max(1, width)
        let height = max(1, height)
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
        let lineCount = max(1, lines.count)
        let usedHeight = min(height, lineCount * 2)
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        var kinds = [UInt8](repeating: 0, count: text.utf16.count)
        for token in highlightDocument(fileName: fileName, text: text) {
            for position in Int(token.startUtf16)..<min(kinds.count, Int(token.endUtf16)) {
                kinds[position] = token.kind
            }
        }
        var lineOffsets = [Int]()
        lineOffsets.reserveCapacity(lineCount)
        var nextOffset = 0
        for line in lines {
            lineOffsets.append(nextOffset)
            nextOffset += line.utf16.count + 1
        }
        let glyphHeight = lineCount * 2 <= height ? 2 : 1
        let rows = glyphHeight == 2 ? lineCount : height
        for row in 0..<rows {
            let lineNumber = glyphHeight == 2 ? row : row * (lineCount - 1) / max(1, rows - 1)
            let line = lines[lineNumber]
            let lineOffset = lineOffsets[lineNumber]
            let y = glyphHeight == 2 ? row * 2 : row
            var x = 4
            for (column, unit) in line.utf16.enumerated() {
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
                    for dy in 0..<glyphHeight where y + dy < height {
                        let coverage = glyphHeight == 1
                            ? max(Self.glyphs[glyphIndex], Self.glyphs[glyphIndex + 1])
                            : Self.glyphs[glyphIndex + dy]
                        if coverage == 0 { continue }
                        let offset = ((y + dy) * width + x) * 4
                        pixels[offset] = rgb.0
                        pixels[offset + 1] = rgb.1
                        pixels[offset + 2] = rgb.2
                        pixels[offset + 3] = UInt8(min(255, Int(coverage) * 2))
                    }
                }
                x += 1
            }
        }
        return Self(width: width, height: height, usedHeight: usedHeight,
                    lineCount: lineCount, pixels: pixels)
    }

    func image() -> NSImage? {
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
