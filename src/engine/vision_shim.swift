// Apple Vision OCR shim for the sofer kernel.
//
// usage: vision_ocr_shim <image>
// prints JSON: {"lines":[{"text":..,"conf":..,"bbox":[x0,y0,x1,y1]}],"ms":..}
//
// Deliberately minimal: `VNRecognizeTextRequest` in accurate mode only.
// `RecognizeDocumentsRequest` (macOS 26) was rejected in docs §4.3⑤ — it drops
// leading indentation and splits code lines, which this project reconstructs
// from pixels instead.
import Foundation
import Vision
import ImageIO

struct Line: Codable {
    let text: String
    let conf: Float
    let bbox: [Double]
}

struct Payload: Codable {
    let lines: [Line]
    let ms: Double
}

func fail(_ message: String) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(2)
}

let args = CommandLine.arguments
guard args.count >= 2 else { fail("usage: vision_ocr_shim <image>") }
let path = args[1]

guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
    fail("cannot load image: \(path)")
}

let started = Date()

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
// Code and terminals: language correction rewrites symbols it considers typos.
request.usesLanguageCorrection = false
request.recognitionLanguages = ["en-US"]
request.automaticallyDetectsLanguage = false
// Keep small monospace glyphs; 0 disables the minimum text height filter.
request.minimumTextHeight = 0.0

let handler = VNImageRequestHandler(cgImage: image, options: [:])
do {
    try handler.perform([request])
} catch {
    fail("vision request failed: \(error)")
}

let width = Double(image.width)
let height = Double(image.height)
var lines: [Line] = []
for observation in request.results ?? [] {
    guard let candidate = observation.topCandidates(1).first else { continue }
    let box = observation.boundingBox
    // Vision uses normalised, bottom-left-origin coordinates; emit pixels with a
    // top-left origin so the Rust side can treat them like any other engine's bbox.
    lines.append(Line(text: candidate.string,
                      conf: candidate.confidence,
                      bbox: [Double(box.minX) * width,
                             (1 - Double(box.maxY)) * height,
                             Double(box.maxX) * width,
                             (1 - Double(box.minY)) * height]))
}

let ms = Date().timeIntervalSince(started) * 1000
let encoder = JSONEncoder()
encoder.outputFormatting = [.sortedKeys]
guard let data = try? encoder.encode(Payload(lines: lines, ms: ms)) else {
    fail("cannot encode result")
}
FileHandle.standardOutput.write(data)
