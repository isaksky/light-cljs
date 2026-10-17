const fs = require('node:fs');
const rt = (0, eval)(fs.readFileSync('src/js_runtime.js', 'utf8'));
globalThis.__lightRuntimeForCompiler = rt;
(0, eval)(fs.readFileSync(process.argv[2], 'utf8'));
console.log((() => {
            const a = {}, b = {}, rawC = {};
            let armed = false, calls = 0;
            const c = new Proxy(rawC, {deleteProperty(target, key) {
                if (armed) {
                    armed = false;
                    void __lightCompilerRoots.b;
                }
                return Reflect.deleteProperty(target, key);
            }});
            for (const [name, value, number] of [['a', a, 1], ['b', b, 2], ['c', c, 3]]) {
                __lightRegisterBundle([name], rt => {
                    calls++;
                    rt.extendInstance(value, [['ICounted', 0], ['ICounted/-count', 1, () => number]], true);
                    __lightCompilerBundleProducts[name] = value;
                }, [['probe', '_', name]], 'probe');
            }
            void __lightCompilerRoots.c;
            void __lightCompilerRoots.a;
            armed = true;
            __lightEnterCompilerFrontier(0);
            const absent = !Object.hasOwn(b, 'cljs$core$ICounted$_count$arity$1');
            __lightLeaveCompilerFrontier();
            return JSON.stringify([absent, calls, rt.core.count(a), rt.core.count(b), rt.core.count(c)]);
        })());
