import SwiftUI

@main
struct UnApp: App {
    init() { unmethod_mobile_init() }

    var body: some Scene {
        WindowGroup { LessonScreen() }
    }
}
