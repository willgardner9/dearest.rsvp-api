# Onboarding: design

Date: 2026-11-25 (updated: five steps in three groups)
Repositories: `dearest.rsvp-web` (frontend only)
Related: [Product vision](../../product/vision.md)

## Intent

Onboarding must orient a new couple and teach them what the product does.
Today onboarding only creates the tenant. The couple types a wedding name,
and the app sends them to a home page that says "More is coming soon."

After this work, the wedding home page explains the five optional steps in
three groups. The
couple understands the product before the features exist, and each
feature has a place to go when it ships.

The steps form a graph, not a sequence. A few steps depend on others, and
a couple can start at any step. For example, a couple can put a website up
quickly and do the rest later, or send save the dates before they finalise
the events. So the guide groups the steps and does not number them.

## Goals

- A new couple learns the five steps and their dependencies.
- The couple learns that the website has a security choice (open,
  password or PIN) before they get to it.
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
- No enforcement of dependencies. The guide only states them.

## Anti-goals

The work is a failure, even if it works, if one of these is true:

- The couple thinks they must do all five steps.
- The couple thinks the steps have a fixed order.
- The couple thinks a feature works today when it does not.
- The guide stores state in `localStorage` or another place that the API
  will replace later.

## Approaches

| Approach | Summary | Result |
| --- | --- | --- |
| A. Overview screen | One extra screen after the name form. The couple sees it once. | Rejected. The explanation is lost after the first visit. |
| B. Step-by-step tour | One screen for each step. | Rejected. Too much to read before the couple gets in. |
| C. Guide on the home page | No extra screen. The home page holds the guide. | **Picked.** The couple sees it on each visit. Each card becomes a link when its feature ships. |

Inside approach C, the steps were first a numbered list. That was rejected,
because a couple can start at any step and the order changes from couple to
couple. The steps are now in groups, with no order inside or between the
groups.

## Design

### Flow

1. A verified user with no memberships goes to `/onboarding`. This is the
   current behavior of `resolveHome`.
2. `/onboarding` stays as it is. It shows the "Wedding name" form
   (`WeddingNameForm`), calls `createTenant` and sends the user to
   `/w/$tenantId`.
3. `WeddingHomePage` shows only a new `WeddingGuide` component in place of
   the text "More is coming soon." The wedding name moves to the header
   (see App shell).

`NewWeddingPage` uses the same form and the same redirect, so a second
wedding gets the same guide.

### `WeddingGuide` content

All content is static. The component uses no state, no API call and no
`localStorage`. It has no intro line and no example paths: the cards and
the groups must speak for themselves.

- **Three groups of step cards.** Each card has a title, a
  one-line explanation, an optional "Needs" line and a "Coming soon" badge.

| Group | Step | Explanation | Needs |
| --- | --- | --- | --- |
| Your wedding | Add your events | The ceremony, the reception, the golf the day after, and the questions guests answer, such as meal choice and dietary needs. | — |
| Your wedding | Build your website | Put your details online. Choose open, password or PIN. PIN is recommended. | — |
| Your guests | Build your guest list | Import your guests, or collect their contact details with a form. Choose who is invited to which event, and see their replies. | — |
| Messages to your guests | Send save the dates | Guests do not need to reply. Send by email or WhatsApp. | Guest list |
| Messages to your guests | Send RSVP requests | Ask guests to reply on your website for each event, and remind the ones who have not. | Events, guest list, website |

An earlier version had a separate "Build your RSVPs" step. It was removed,
because the RSVP questions belong to an event (a meal choice applies to the
dinner, not to the golf), and who is invited to which event belongs to the
guest list. The replies show in the guest list. The "Send RSVP requests"
page only sends the requests and the reminders.

### Text sketch

```
dearest.rsvp | /to/will-and-julia

Your wedding
  Add your events                                Coming soon
  Build your website                             Coming soon

Your guests
  Build your guest list                          Coming soon

Messages to your guests
  Send save the dates                            Coming soon
    Needs: guest list
  Send RSVP requests                             Coming soon
    Needs: events, guest list, website
```

### App shell

- **Header:** the logo, then the website address `/to/<slug>`. A planner
  with many weddings sees which wedding is open. Until the couple chooses a
  slug, the app makes one from the wedding name (see the product vision).
- **Navigation:** the five steps, under the group names Wedding, Guests and
  Messages, as disabled items with a "Soon" badge. They read the same data
  as the guide, so the two always match.

## Left to the builder

These are the builder's calls:

- The exact wording of each card. Keep the meaning above.
- The Mantine components for the groups, the cards, the badge and the
  collapsible section.
- The test approach for the new component.
