// The Swift bridging header: the UniFFI scaffolding plus the product library's init function.
#include "../Generated/un_ffiFFI.h"

/// Makes the unmath lessons the app's lesson list (un-mobile). Absent when linking un-ffi alone,
/// in which case drop the call in UnApp.swift.
void un_mobile_init(void);
