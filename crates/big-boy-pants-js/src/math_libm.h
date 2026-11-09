/* The platform's libm functions Math needs and Goose's `math` module lacks
   (light-cljs-901; math_object.goose's `extern fn libm_*`): sinh, cosh,
   tanh, asinh, acosh, atanh, expm1, log1p and cbrt, as both reference
   engines call them on this machine (QuickJS directly, the Pants engine
   through Rust's std or its own libm bindings). Each is a plain call into
   libm; none touches Goose storage or runs JavaScript.

   `goose run` (in-process TinyCC, the `--debug` runs without `build`) has
   no C library math beyond the runtime's own, so there goose_math_libm()
   is 0 and math_object.goose fails such a call by name before it would
   use the NaN these return; a program `goose build` made links libm. */
#if defined(__TINYC__)
static uint8_t goose_math_libm(void) { return 0; }
static double goose_math_sinh(double x) { (void)x; return 0.0 / 0.0; }
static double goose_math_cosh(double x) { (void)x; return 0.0 / 0.0; }
static double goose_math_tanh(double x) { (void)x; return 0.0 / 0.0; }
static double goose_math_asinh(double x) { (void)x; return 0.0 / 0.0; }
static double goose_math_acosh(double x) { (void)x; return 0.0 / 0.0; }
static double goose_math_atanh(double x) { (void)x; return 0.0 / 0.0; }
static double goose_math_expm1(double x) { (void)x; return 0.0 / 0.0; }
static double goose_math_log1p(double x) { (void)x; return 0.0 / 0.0; }
static double goose_math_cbrt(double x) { (void)x; return 0.0 / 0.0; }
#else
#include <math.h>
static uint8_t goose_math_libm(void) { return 1; }
static double goose_math_sinh(double x) { return sinh(x); }
static double goose_math_cosh(double x) { return cosh(x); }
static double goose_math_tanh(double x) { return tanh(x); }
static double goose_math_asinh(double x) { return asinh(x); }
static double goose_math_acosh(double x) { return acosh(x); }
static double goose_math_atanh(double x) { return atanh(x); }
static double goose_math_expm1(double x) { return expm1(x); }
static double goose_math_log1p(double x) { return log1p(x); }
static double goose_math_cbrt(double x) { return cbrt(x); }
#endif
