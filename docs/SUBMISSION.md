# Bonded — hackathon submission checklist

Source of truth: <https://nearhackathon.com/HACKATHON.md>. Read it again before
submitting; dates and rules can change.

## Before submitting

- [ ] Repository is public and contains no secrets (`.env`, keys, credentials).
- [ ] `README.md` explains what is real, deterministic, and not included.
- [ ] Contract is deployed and the address in the README matches the submission.
- [ ] Demo post is public on X, says “built at NEAR HACKATHON”, tags
      `@NEARProtocol` and `#NEARHackathon`, and includes the video/screenshots.
- [ ] The demo still works (or the video is the reliable fallback).
- [ ] The submission payload has been printed and confirmed by the human.

## Suggested payload

Replace every placeholder with real values. Do not submit this file as-is.

```json
{
  "requestId": "<uuid generated once and reused on retries>",
  "projectName": "Bonded",
  "leadName": "<team lead>",
  "email": "<contact email>",
  "size": "1",
  "tracks": ["agents", "security", "business"],
  "nearAddress": "<mainnet NEAR account for the prize>",
  "projectUrl": "<public repository URL>",
  "demoUrl": "<public X post URL>",
  "idea": "AI agents fail often and lose nothing when they do. Bonded is a performance-bond layer: a buyer funds a job in a NEAR contract, the agent locks collateral, and a verifiable acceptance test decides whether the agent is paid or the bond is slashed to the buyer. The first wedge is code-fix bounties with a pinned test suite; TypeSafe Jev reviews passing patches for test manipulation. The demo shows a real test run, a real bond, and a real slash on NEAR testnet.",
  "consent": true
}
```

## Tracks

- `agents` — primary: the trust/collateral layer for agents that earn and spend.
- `security` — capital-backed reputation and a defense against test manipulation.
- `business` — an AI-native business with a treasury, bond, and enforced
  constraints.

## After submitting

- Keep the repository public through judging (at least 18 Nov 2026).
- Keep the demo hosted or publish the video; the judge should be able to see the
  loop without reverse-engineering the repo.
- Save the submission response and the demo post URL.

## Do not

- Add a `website` field to the API payload; it is a spam trap.
- Submit twice for the same repository.
- Post on X or send the submission without the human's explicit confirmation.
- Commit `.env`, private keys, or personal data.
