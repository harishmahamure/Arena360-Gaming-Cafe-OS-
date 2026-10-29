-- 0066: an order marked ready (Story 6.33). `order.ready` stamps the open bill with the moment
-- staff said the food is ready; placing more lines on the bill clears it again. The kiosk reads it
-- through the session tick (`ready` is the branch's fulfilment, seat or counter, while it is set).
ALTER TABLE orders ADD COLUMN ready_at INTEGER NULL;
