// The Swift bridging header: the UniFFI scaffolding plus the product library's init function.
#include "../Generated/un_ffiFFI.h"

/// Registers the Unmethod lessons (unmethod-mobile). Absent when linking un-ffi alone, in
/// which case drop the call in UnApp.swift.
void unmethod_mobile_init(void);

// Debug: times the bundled language model on the tutor's master prompt (UN_LLM_BENCH=1).
void unmethod_bench_chat(const char *path);
