import CoreGraphics
import Foundation

let opts: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
guard let list = CGWindowListCopyWindowInfo(opts, kCGNullWindowID) as? [[String: Any]] else {
    exit(1)
}
for w in list {
    let owner = w[kCGWindowOwnerName as String] as? String ?? "?"
    let num = w[kCGWindowNumber as String] as? Int ?? 0
    let layer = w[kCGWindowLayer as String] as? Int ?? -1
    let b = w[kCGWindowBounds as String] as? [String: CGFloat] ?? [:]
    let x = b["X"] ?? 0, y = b["Y"] ?? 0, ww = b["Width"] ?? 0, hh = b["Height"] ?? 0
    if layer == 0 && ww > 200 {
        print("\(num)\t\(owner)\t\(Int(x))\t\(Int(y))\t\(Int(ww))\t\(Int(hh))")
    }
}
