# Review ratchets

One line per defect class found in a past review. Check every line against each candidate.
Append a new line whenever a review finds a defect class that is not already listed; where
the check is mechanical, also add it to `config/gates.json`.

- Provider errors must fall back to the existing legacy parse/rate flow; never let a provider failure silently complete a BullMQ job.
- Never pass a raw SDK error object with an enumerable body to Sentry; sanitise to intent, status, and requestId only.
