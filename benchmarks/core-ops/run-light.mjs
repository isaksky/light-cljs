// Runs light-cljs's release build of core-ops.main, as `node out/shadow.js`
// runs shadow-cljs's: node run-light.mjs SEED ROUNDS OPS SCALE. run.bb's
// --pseudo-names mode sets CORE_OPS_LIGHT_OUT=out/light-profile.
const out = process.env.CORE_OPS_LIGHT_OUT ?? 'out/light';
const { default: main } = await import(new URL(`./${out}/core-ops.main.js`, import.meta.url));
main(...process.argv.slice(2));
