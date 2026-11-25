# Onboarding: design

Date: 2026-11-25
Repositories: `dearest.rsvp-web` (frontend only)
Related: [Product vision](../../product/vision.md)

## Intent

Onboarding must orient a new couple and teach them what the product does.
Today onboarding only creates the tenant. The couple types a wedding name,
and the app sends them to a home page that says "More is coming soon."

After this work, the wedding home page explains the three optional steps:
guest list, save the dates and RSVP. It also shows the example paths that
couples follow. The couple understands the product before the features
exist, and each feature has a place to go when it ships.

## Goals

- A new couple learns the three optional steps and the example paths.
- The couple learns that RSVP has a site security choice (open, password
  or PIN) before they get to it.
- Every member of a wedding sees the same guide. This includes an invited
  partner or planner.
- Each step has one place on the home page. A future feature replaces the
  "Coming soon" badge with a link.

## Non-goals

- No API change. No new tenant fields. No migration.
- No wedding date, location or events in onboarding. These belong to
  `wedding_events`, which is a separate feature.
- No question about which path the couple wants. Nothing exists yet that
  the answer could change, and a web-only answer is lost on another device.
- No "done" state for a step and no way to hide the guide. Both need data
  that does not exist yet.

## Anti-goals

The work is a failure, even if it works, if one of these is true:

- The couple thinks they must do all three steps.
- The couple thinks a feature works today when it does not.
- The guide stores state in `localStorage` or another place that the API
  will replace later.

## Approaches

| Approach | Summary | Result |
| --- | --- | --- |
| A. Overview screen | One extra screen after the name form. The couple sees it once. | Rejected. The explanation is lost after the first visit. |
| B. Step-by-step tour | One screen each for guest list, save the dates, RSVP and site security. | Rejected. Too much to read before the couple gets in. |
| C. Guide on the home page | No extra screen. The home page holds the guide. | **Picked.** The couple sees it on each visit. Each card becomes a link when its feature ships. |

## Design

### Flow

1. A verified user with no memberships goes to `/onboarding`. This is the
   current behavior of `resolveHome`.
2. `/onboarding` stays as it is. It shows the "Wedding name" form
   (`WeddingNameForm`), calls `createTenant` and sends the user to
   `/w/$tenantId`.
3. `WeddingHomePage` shows the wedding name. A new `WeddingGuide`
   component replaces the text "More is coming soon."

`NewWeddingPage` uses the same form and the same redirect, so a second
wedding gets the same guide.

### `WeddingGuide` content

All content is static. The component uses no state, no API call and no
`localStorage`.

- **Intro line:** "Each step is optional. Do the ones that fit you."
- **Three step cards.** Each card has a title, a one-line explanation and
  a "Coming soon" badge:
  1. **Build your guest list.** Import your guests, or send a
     contact-details form that anyone can fill in. You then sort the
     replies into your guest list.
  2. **Send save the dates.** Guests do not need to reply. Send them by
     email or WhatsApp.
  3. **Collect RSVPs.** Guests reply for each event. You choose the site
     security: open, password or PIN. PIN is recommended.
- **"Example paths" section.** It is collapsed by default and opens on a
  click. It shows:
  - Import guests, then send RSVPs.
  - Import guests, send save the dates, then send RSVPs months later.
  - Collect contact details, send save the dates, then send RSVPs.

### Text sketch

```
Will & Julia

Each step is optional. Do the ones that fit you.

┌──────────────────────────────────────────────┐
│ Build your guest list            Coming soon │
│ Import guests, or collect their details.     │
└──────────────────────────────────────────────┘
┌──────────────────────────────────────────────┐
│ Send save the dates              Coming soon │
│ No reply needed. Email or WhatsApp.          │
└──────────────────────────────────────────────┘
┌──────────────────────────────────────────────┐
│ Collect RSVPs                    Coming soon │
│ Choose open, password or PIN (recommended).  │
└──────────────────────────────────────────────┘

▸ See example paths
```

## Left to the builder

These are the builder's calls:

- The exact wording of each card. Keep the meaning above.
- The Mantine components for the cards, the badge and the collapsible
  section.
- The test approach for the new component.
- Add `.superpowers/` to the `.gitignore` of `dearest.rsvp-web`.
