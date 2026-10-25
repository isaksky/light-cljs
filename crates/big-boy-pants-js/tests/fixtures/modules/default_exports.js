// Synthetic fixture, not light-cljs output.
export default function original() { return original === replacement ? 2 : 1; }
export {original};
export function replace() { original = replacement; }
function replacement() { return 2; }
