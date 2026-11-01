// Apple Vision OCR harness for the sofer benchmark.
// usage: ocr_vision <image> [--fast] [--correction] [--doc]
//   --fast        : VNRequestTextRecognitionLevel.fast
//   --correction  : enable usesLanguageCorrection (default off, better for code)
//   --doc         : use macOS 26 RecognizeDocumentsRequest (structured, with tables)
// prints JSON: {"lines":[{"text":..,"conf":..,"bbox":[x,y,w,h]}],"ms":..}
import Foundation
import Vision
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

struct Line: Codable { let text: String; let conf: Float; let bbox: [Double] }

func loadCGImage(_ path: String) -> CGImage? {
    guard let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let img = CGImageSourceCreateImageAtIndex(src, 0, nil) else { return nil }
    return img
}

let args = CommandLine.arguments
guard args.count >= 2, let cg = loadCGImage(args[1]) else {
    FileHandle.standardError.write("cannot load image\n".data(using: .utf8)!)
    exit(2)
}
let fast = args.contains("--fast")
let correction = args.contains("--correction")
let useDoc = args.contains("--doc")

let t0 = Date()

final class Box: @unchecked Sendable { var lines: [Line] = [] }

func runDoc(_ img: CGImage) async -> [Line] {
    if #available(macOS 26.0, *) {
        let req = RecognizeDocumentsRequest()
        let obs = try? await req.perform(on: img)
        guard let doc = obs?.first?.document else { return [] }
        var out: [Line] = []
        for line in doc.text.lines {
            out.append(Line(text: String(line.transcript), conf: 1.0, bbox: [0, 0, 0, 0]))
        }
        return out
    }
    return []
}

func runText() -> [Line] {
    let req = VNRecognizeTextRequest()
    req.recognitionLevel = fast ? .fast : .accurate
    req.usesLanguageCorrection = correction
    req.recognitionLanguages = ["en-US"]
    req.automaticallyDetectsLanguage = false
    // Maximise recall on small monospace glyphs: no minimum text height, keep default.
    req.minimumTextHeight = 0.0
    let handler = VNImageRequestHandler(cgImage: cg, options: [:])
    do { try handler.perform([req]) } catch { return [] }
    guard let results = req.results else { return [] }
    var out: [Line] = []
    for o in results {
        guard let c = o.topCandidates(1).first else { continue }
        let bb = o.boundingBox
        let W = Double(cg.width), H = Double(cg.height)
        // normalised, bottom-left origin -> pixel, top-left origin
        out.append(Line(text: c.string, conf: c.confidence,
                        bbox: [Double(bb.minX) * W, (1 - Double(bb.maxY)) * H,
                               Double(bb.maxX) * W, (1 - Double(bb.minY)) * H]))
    }
    // reading order: top-to-bottom (Vision y is bottom-up), then left-to-right
    out.sort { a, b in
        if abs(a.bbox[1] - b.bbox[1]) > 3 { return a.bbox[1] < b.bbox[1] }
        return a.bbox[0] < b.bbox[0]
    }
    return out
}

var lines: [Line] = []
if useDoc {
    let box = Box()
    let sem = DispatchSemaphore(value: 0)
    Task { box.lines = await runDoc(cg); sem.signal() }
    sem.wait()
    lines = box.lines
} else {
    lines = runText()
}
let ms = Date().timeIntervalSince(t0) * 1000
let payload: [String: Any] = [
    "engine": useDoc ? "vision-doc" : (fast ? "vision-fast" : "vision-accurate"),
    "correction": correction,
    "ms": ms,
    "lines": lines.map { ["text": $0.text, "conf": $0.conf, "bbox": $0.bbox] },
]
let data = try! JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])
FileHandle.standardOutput.write(data)
