#!/usr/bin/env node
// Current product visual review. Historical layouts remain in Git history.
if (process.env.NPC2_PRODUCT_VISUAL_OUTPUT && !process.env.NPC2_UI_REVIEW_OUTPUT) {
  process.env.NPC2_UI_REVIEW_OUTPUT = process.env.NPC2_PRODUCT_VISUAL_OUTPUT;
}
process.env.NPC2_UI_URL ||= 'http://127.0.0.1:1420';
require('./cyberpunk-ui-review.cjs');
