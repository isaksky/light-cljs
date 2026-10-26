(ns rec.macros)
(defprotocol Endpoint (address [e]))
(defrecord Server [port host] Endpoint (address [_] [port host]))
(def default-server (->Server 8080 "localhost"))
(defn secure [server] (assoc (assoc server :port 9090) :host :tls))
(defmacro endpoint [secure?]
  (list 'quote (address (if secure? (secure default-server) default-server))))
