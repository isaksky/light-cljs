(ns light.build
  "Compile-time services for macros, the helper code they run and custom data
  readers (docs/compiler.md, \"Compile-time services: light.build\"). Each read
  or declaration is a dependency of the namespace being compiled, which a
  compiler cache keys on. Paths resolve like namespaces, across every source
  root, and a path found in two roots is an error. A path starting with `./`
  or `../` resolves against the directory of the file of the namespace whose
  macro expansion is running. `config` answers the build configuration
  (`--config KEY=EDN`), and `depend-on!` declares a coarse dependency on a
  broad compiler view. They work only at compile time: in emitted
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

(defn config
  "The value of `key`, a keyword, in the build configuration (`--config
  KEY=EDN`), or nil when the key is unset. Every namespace of a build
  depends on the whole configuration, whether or not it reads a key, so
  changing any key recompiles every namespace a compiler cache holds."
  [key]
  (let [build (service "config")]
    (if (keyword? key)
      (when-some [text (.config build (subs (str key) 1))]
        (compiler-edn-read-string text))
      (throw (js/Error. (str "light.build/config: expected a keyword, got " (pr-str key)))))))

(defn depend-on!
  "Declare a coarse dependency on a broad compiler view, for reflective
  macros such as test discovery, and return nil. `view` is `:namespaces`,
  the build's namespace inventory (names and resolved source identities);
  `:declarations`, the declarations of every namespace earlier in source
  order; or `:config`, the build configuration's fingerprint."
  [view]
  (let [build (service "depend-on!")]
    (if (contains? #{:namespaces :declarations :config} view)
      (.dependOn build (name view))
      (throw (js/Error. (str "light.build/depend-on!: expected :namespaces, :declarations"
                             " or :config, got " (pr-str view)))))
    nil))
