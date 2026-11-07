// ThrowError messages (light-cljs-824): the compiler's static throws carry
// the Pants engine's wording, naming the binding by its source text, a
// non-ASCII identifier included. Completes with true.
const café = 1;
var messages = "";
try { café = 2; } catch (e) { messages = messages + e.name + ":" + e.message + ";"; }
try { typeof later; } catch (e) { messages = messages + e.name + ":" + e.message + ";"; }
let later = 3;
messages === "TypeError:assignment to constant variable 'café';ReferenceError:cannot access 'later' before initialization;";
