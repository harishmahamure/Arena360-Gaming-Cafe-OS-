-- What a stored change of a subscription is about (Story 10.10). `plan` is a change of tier and
-- add-ons (Story 10.9). `cycle` switches between monthly and yearly: Razorpay cannot change a
-- subscription's period, so it is always a new Razorpay subscription (`razorpay_subscription_id`)
-- that the owner authorises first; `cycle` is the cycle it switches to. `method` replaces the
-- mandate only: the plan, cycle and amounts stay as they are.
ALTER TABLE subscription_changes ADD COLUMN subject TEXT NOT NULL DEFAULT 'plan' CHECK (subject IN ('plan', 'cycle', 'method'));

ALTER TABLE subscription_changes ADD COLUMN cycle TEXT CHECK (cycle IS NULL OR cycle IN ('monthly', 'yearly'));
