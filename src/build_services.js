// The compile-time services of light.build over one string service (see
// src/build_services.rs): `service(request)` and `caller()`, which describes
// the code running. light.build's functions and custom data readers call it.
(service, caller) => {
  const call = (operation, first, second) => {
    const answer = service(caller() + "\u0000" + operation + "\u0000" + first + "\u0000" + second);
    if (answer[0] === "v") return answer.slice(1);
    if (answer[0] === "n") return null;
    throw new Error(answer.slice(1));
  };
  // Requests carry text, so both engines' bridges see the same request:
  // NUL separates its fields, and a lone surrogate has no UTF-8 form (V8
  // would replace it, QuickJS refuse it), so neither may appear.
  const text = (name, value) => {
    if (typeof value !== "string")
      throw new TypeError("light.build/" + name + ": expected a string, got " + (value === null ? "nil" : typeof value));
    for (let i = 0; i < value.length; i++) {
      const unit = value.charCodeAt(i);
      if (unit === 0) throw new TypeError("light.build/" + name + ": a string with a NUL character");
      if (unit >= 0xd800 && unit <= 0xdfff) {
        const next = value.charCodeAt(i + 1);
        if (unit <= 0xdbff && next >= 0xdc00 && next <= 0xdfff) { i++; continue; }
        throw new TypeError("light.build/" + name + ": a string with a lone surrogate");
      }
    }
    return value;
  };
  return Object.freeze({
    resource: path => call("resource", text("resource", path), ""),
    optionalResource: path => call("resource?", text("resource?", path), ""),
    dependResource: path => call("depend-resource", text("depend!", path), ""),
    dependNamespace: name => call("depend-namespace", text("depend!", name), ""),
    dependProvider: (token, fingerprint) => call("depend-provider", text("depend!", token), text("depend!", fingerprint)),
    // The canonical EDN text of a key's value, or null when it is unset.
    config: key => call("config", text("config", key), ""),
    dependOn: view => call("depend-on", text("depend-on!", view), "")
  });
}
