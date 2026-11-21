Use babashka (bb) instead of python wherever possible, especially for scripts you check in to this repository. Some scripts are already in python, but that is because I wrote this message too late. Try to use babashka from now on.

Some of our goals:

  - Lower memory than shadow-cljs
  - Lower build times (both cached and uncached)
  - As small or smaller advanced compilation output size
  - As fast or faster output load times and steady state performance
  - Lower complexity (since we we will just have one runtime language to reason about)

Some of our non-goals:

- Being completely compatible with ClojureScript (shadow-cljs). This is because we do not want to emulate the JVM, and we don't want to to include the google closure library. Those are complexities we want to avoid in this project.
