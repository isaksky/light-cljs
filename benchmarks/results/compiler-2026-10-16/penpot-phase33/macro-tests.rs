#[test]
fn macro_arguments_share_form_elements_and_mutations() {
    // Upstream macroexpand-1* applies the expander to form, env and (rest form).
    let source = r#"
      (defmacro same [a b c d & more]
        (list 'quote [(identical? a (nth &form 1))
                      (identical? b (nth &form 2))
                      (identical? c (nth &form 3))
                      (identical? d (nth &form 4))
                      (identical? (first more) (nth &form 5))]))
      (defmacro rename-argument [value]
        (set! (.-name value) "changed")
        (second &form))
      (pr-str [(same [1] {:key 2} #js [3] #"x" unresolved)
               (let [original 1 changed 2] (rename-argument original))])"#;
    let mut host = MacroHost::new();
    for _ in 0..2 {
        assert_eq!(result(&compile_with_macros(source, &mut host).unwrap()), "[[true true true true true] 2]\n");
    }
}
