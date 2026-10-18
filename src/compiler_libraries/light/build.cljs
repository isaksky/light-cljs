(ns light.build
  "Compile-time services for macros, the helper code they run and custom data
  readers (docs/compiler.md, \"Compile-time services: light.build\"). Each read
  or declaration is a dependency of the namespace being compiled, which a
  compiler cache keys on. Paths resolve like namespaces, across every source
  root, and a path found in two roots is an error. A path starting with `./`
  or `../` resolves against the directory of the file of the namespace whose
  macro expansion is running. They work only at compile time: in emitted
  application code, every call is an error.")

(defn- service
  [name]
  (if (exists? js/lightCljsBuild)
    js/lightCljsBuild
    (throw (js/Error. (str "light.build/" name
                           " is only available at compile time: call it from a macro,"
                           " a helper a macro runs or a custom data reader")))))

(defn resource
  "The text of resource `path`. An absent resource is an error."
  [path]
  (.resource (service "resource") path))

(defn resource?
  "The text of resource `path`, or nil when it is absent. Absence is a
  dependency too: adding the resource invalidates the result."
  [path]
  (.optionalResource (service "resource?") path))

(defn depend!
  "Declare a dependency without reading it, and return nil:
  `{:resource path}` on a resource, absent or not; `{:namespace sym}` on a
  namespace's source; or `{:provider token :fingerprint string}` on a host
  provider's current state, which `token` names and `fingerprint` describes."
  [dependency]
  (let [build (service "depend!")
        shape (when (map? dependency) (set (keys dependency)))]
    (cond
      (= shape #{:resource}) (.dependResource build (:resource dependency))
      (= shape #{:namespace})
      (let [namespace (:namespace dependency)]
        (if (symbol? namespace)
          (.dependNamespace build (str namespace))
          (throw (js/Error. "light.build/depend!: :namespace takes a symbol"))))
      (= shape #{:provider :fingerprint})
      (let [token (:provider dependency)]
        (.dependProvider build
                         (if (or (keyword? token) (symbol? token)) (str token) token)
                         (:fingerprint dependency)))
      :else
      (throw (js/Error. (str "light.build/depend!: expected {:resource path}, {:namespace sym}"
                             " or {:provider token :fingerprint string}, got "
                             (pr-str dependency)))))
    nil))
