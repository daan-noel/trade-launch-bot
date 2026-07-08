-- Collapse the redundant `_devbuy` launch variants. The dev-buy was never
-- encoded in the variant string — it's driven purely by the template's
-- `dev_buy_quote` (> 0 ⇒ create+dev-buy, else plain create), so
-- `pumpfun.create_v2` and `pumpfun.create_v2_devbuy` (and the v1 pair) always
-- behaved identically. launcher::service::execute_launch now matches only the
-- base variants; rewrite any existing rows so they still dispatch instead of
-- hitting the `unsupported launch variant` bail.
--
-- `variant` is free TEXT (no CHECK), so this is a plain value rewrite. Both the
-- authoring table and the immutable launch record are normalized so no
-- `_devbuy` spelling survives anywhere.

UPDATE launch_templates
SET variant = replace(variant, '_devbuy', '')
WHERE variant LIKE '%\_devbuy' ESCAPE '\';

UPDATE launches
SET variant = replace(variant, '_devbuy', '')
WHERE variant LIKE '%\_devbuy' ESCAPE '\';
