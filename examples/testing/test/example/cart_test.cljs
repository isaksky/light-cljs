(ns example.cart-test
  (:require [cljs.test :refer [deftest is testing async are]]
            [example.cart :as cart]))

(deftest add-item-merges-quantities
  (let [cart (-> {}
                 (cart/add-item :apple 3 2)
                 (cart/add-item :apple 3 1))]
    (is (= {:apple {:price 3 :quantity 3}} cart))))

(deftest total-sums-lines
  (testing "an empty cart"
    (is (zero? (cart/total {}))))
  (are [cart expected] (= expected (cart/total cart))
    {:a {:price 2 :quantity 3}} 6
    {:a {:price 2 :quantity 3} :b {:price 5 :quantity 1}} 11))

;; A timer is what the embedded V8 heap of `light-cljs test` does not have:
;; this test needs Node or a browser.
(deftest price-arrives-later
  (async done
    (cart/price-later {:pear 4} :pear 10
                      (fn [price]
                        (is (= 4 price))
                        (done)))))
