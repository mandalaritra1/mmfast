// Prints the Unix time (s) when `owner` first shows an on-screen window taller than 300 pt.
import Foundation
import CoreGraphics
let owner = CommandLine.arguments[1]
let timeout = Double(CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : "30") ?? 30
let start = Date()
while Date().timeIntervalSince(start) < timeout {
    if let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] {
        for w in list where (w[kCGWindowOwnerName as String] as? String) == owner && (w[kCGWindowLayer as String] as? Int) == 0 {
            if let b = w[kCGWindowBounds as String] as? [String: Any], let h = b["Height"] as? Double, h >= 600, let wd = b["Width"] as? Double, wd >= 700 {
                print(Date().timeIntervalSince1970); exit(0)
            }
        }
    }
    usleep(4000)
}
print("timeout"); exit(1)
