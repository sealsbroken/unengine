// The Swift bridging header: the UniFFI scaffolding plus the product library's init function.
#include "../Generated/un_ffiFFI.h"

// sherpa-onnx's C API, for the Russian tutor's Piper voice (iPhone and iPad; tools/ios.sh stages
// the header in Frameworks/include).
#if __has_include("sherpa-onnx/c-api/c-api.h")
#include "sherpa-onnx/c-api/c-api.h"
#endif

/// Makes the unmath lessons the app's lesson list (un-mobile). Absent when linking un-ffi alone,
/// in which case drop the call in UnApp.swift.
void un_mobile_init(void);
