(require '[babashka.fs :as fs] '[clojure.string :as str])
(def base "/private/tmp/light-cljs-phase13-storage-profile/src")
(defn edit [file before after]
  (let [path (fs/file base file) text (slurp path)]
    (assert (str/includes? text before) file)
    (spit path (str/replace text before after))))
(edit "layered.rs" "    pub fn len(&self) -> usize {"
      "    pub(crate) fn profile_reader_storage(&self, stats: &mut crate::profile::ArenaStorage, mut value: impl FnMut(&V, &mut crate::profile::ArenaStorage), key: impl Fn(&K) -> usize) {
        let row=stats.tables.entry(\"reader_context_layers\").or_default();
        row.0+=self.own.len(); row.1+=self.own.capacity()*std::mem::size_of::<(K, Option<V>)>()+self.own.keys().map(&key).sum::<usize>(); row.2+=1;
        for v in self.own.values().flatten() {value(v,stats);}
        if stats.seen.insert(Rc::as_ptr(&self.base) as usize) {
            let row=stats.tables.entry(\"reader_context_bases\").or_default();
            row.0+=self.base.len();row.1+=self.base.capacity()*std::mem::size_of::<(K,V)>()+self.base.keys().map(key).sum::<usize>();row.2+=1;
            for v in self.base.values() {value(v,stats);}
        }
    }
    pub fn len(&self) -> usize {")
(edit "profile.rs" "        use std::mem::size_of;"
      "        use std::mem::size_of;
        if self.seen.insert(&*p.reader_contexts as *const _ as usize) {
            p.reader_contexts.profile_reader_storage(self, |value,stats| stats.shared(\"reader_context_values\", value, |v| {
                (1, v.aliases.capacity()*size_of::<(String,String)>() + v.aliases.iter().map(|(a,b)|a.capacity()+b.capacity()).sum::<usize>()
                    +v.refers.capacity()*size_of::<(String,String,String)>() +v.refers.iter().map(|(a,b,c)|a.capacity()+b.capacity()+c.capacity()).sum::<usize>()
                    +v.excludes.capacity()*size_of::<String>()+v.excludes.iter().map(String::capacity).sum::<usize>()
                    +v.macros.capacity()*size_of::<String>()+v.macros.iter().map(String::capacity).sum::<usize>())
            }), String::capacity);
        }")
