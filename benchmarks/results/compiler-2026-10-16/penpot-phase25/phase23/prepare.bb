(require '[clojure.string :as str])
(def here "/private/tmp/light-cljs-phase23/")
(def runtime-source (slurp "src/js_runtime.js"))
(def begin (str/index-of runtime-source "  //@unit publicMethod requires:"))
(def end (str/index-of runtime-source "  //@unit ownProtocol requires:" begin))
(assert (and begin end))
(def replacement "  //@unit publicMethodInfo requires: coreProtocolNames, coreProtocolsByName,
  //@ nativeImplementations, protocolMasks, protocolSlot
  function publicMethodInfo(key) {
    let info = publicMethodInfos.get(key);
    if (info === undefined) {
      const protocol = coreProtocolsByName.get(key.split(\"/\")[0]);
      const name = coreProtocolNames.get(protocol);
      info = {protocol, name, mask: protocolMasks.get(name),
        marker: name && protocolSlot(name, 0), native: nativeImplementations.get(protocol), slots: []};
      publicMethodInfos.set(key, info);
    }
    return info;
  }
  const publicMethodInfos = new Map();
  //@unit publicMethod requires: demandMethod, IMeta, protocolHooks, protocolSentinel,
  //@ protocolSlot, publicMethodInfo, satisfies
  function publicMethod(value, key, arity, fast = false, directOnly = false) {
    const info = publicMethodInfo(key), protocol = info.protocol, mask = info.mask, native = info.native;
    let direct = !!(mask && (value?.[mask[0]] & mask[1]));
    if (!direct && info.name) {
      demandMethod(info.name, 0, value);
      direct = value != null && value[info.marker] === protocolSentinel;
    }
    if (!direct) direct = !!native?.(value);
    if (!direct && (directOnly || (fast && (Array.isArray(value) || typeof value === \"string\")))) return undefined;
    if (!direct && !satisfies(protocol, value)) return undefined;
    let slot = info.slots[arity];
    if (slot === undefined) info.slots[arity] = slot = protocolSlot(key, arity);
    demandMethod(key, arity, value);
    let method = value == null ? undefined : value[slot];
    if (method == null) method = protocolHooks.externalMethod?.(value, key, arity);
    if (method) return method;
    if (protocol === IMeta && typeof value === \"function\") return undefined;
    if (!native?.(value)) throw new Error(\"No protocol method \" + key + \" defined for this value\");
    return undefined;
  }")
(spit (str here "before.js") runtime-source)
(spit (str here "after.js") (str (subs runtime-source 0 begin) replacement (subs runtime-source end)))
