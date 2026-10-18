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
  const text = (name, value) => {
    if (typeof value !== "string" || value.includes("\u0000"))
      throw new TypeError("light.build/" + name + ": expected a string, got " + (value === null ? "nil" : typeof value));
    return value;
  };
  return Object.freeze({
    resource: path => call("resource", text("resource", path), ""),
    optionalResource: path => call("resource?", text("resource?", path), ""),
    dependResource: path => call("depend-resource", text("depend!", path), ""),
    dependNamespace: name => call("depend-namespace", text("depend!", name), ""),
    dependProvider: (token, fingerprint) => call("depend-provider", text("depend!", token), text("depend!", fingerprint))
  });
}
