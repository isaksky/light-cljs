(ns views.m) (defmacro probe [] [(some? (find-ns 'views.x)) (some? (find-ns 'views.m)) (contains? (set (map ns-name (all-ns))) 'views.x)])
