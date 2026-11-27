(ns example.cart)

(defn add-item
  "The cart with `quantity` more of the item `sku` at `price` each."
  [cart sku price quantity]
  (update cart sku (fn [line]
                     {:price price
                      :quantity (+ quantity (:quantity line 0))})))

(defn total
  "The cart's total, the sum of each line's price times its quantity."
  [cart]
  (reduce + 0 (map (fn [{:keys [price quantity]}] (* price quantity)) (vals cart))))

(defn price-later
  "Call `callback` with the price of `sku` from `prices` after `millis`
  milliseconds, as a slow service would."
  [prices sku millis callback]
  (js/setTimeout #(callback (get prices sku)) millis))
