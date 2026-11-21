// Runs light-cljs's release build of extend-ops.main:
//   node run-light.mjs SEED ROUNDS OPS SCALE
const out = process.env.EXTEND_OPS_LIGHT_OUT ?? "out/light";
const { default: main } = await import(new URL(`./${out}/extend-ops.main.js`, import.meta.url));
main(...process.argv.slice(2));
