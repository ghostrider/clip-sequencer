# Themes

The app's colours and fonts come from a **theme**. Choose one in **File → Preferences… → Appearance**. Clicking
the clock chip in the status bar at the bottom opens Preferences too, on the *MIDI clock* tab.

## Choosing a theme

Each theme is a card with its name, a sample and its colours. Click a card and the whole app changes at once, so
you can judge it on your real sequences. **Save** keeps it; **Cancel** (or Esc) goes back to the theme you had.

Five themes come with the app:

| Theme | Look |
|---|---|
| **Rack** (default) | Cool slate with a cyan accent, square corners, condensed labels |
| **Ink & Signal** | Deep navy with a hot pink accent |
| **Graphite & Lime** | Graphite grey with a lime accent, no rounded corners |
| **Classic Amber** | The app's earlier look: warm amber, round corners |
| **Daylight** | Light background, for bright rooms and daylight gigs |

The colours keep their meaning in every theme: the **accent** colour marks what runs, what is selected and the
playhead; **info** marks what Resolume has confirmed; **OK** the connections; **error** what is unresolved,
yielded or disconnected.

## Making your own

Click **Change colours and fonts…** under the cards. The editor shows:

- **Colours**, grouped as above (surfaces, lines, text, accent, info, OK, error). Pick a colour or type it as hex,
  `#RRGGBB`, or `#RRGGBBAA` with transparency.
- **Fonts** for four roles: *Text*, *Labels (uppercase)*, *Sequence title* and *Numbers, names, positions*. All
  fonts are bundled with the app, so nothing is downloaded at the venue.

Every change shows at once. Then:

- **Save as new theme:** give it a name; it is saved and selected.
- **Save changes:** for a theme you saved before, keeps the changes under the same name.
- **Discard changes:** back to the theme as it was.
- **Delete theme…** removes a theme you saved (the five built-in ones can't be changed or deleted).

While there are unsaved changes, the dialog's **Save** is greyed out: save them as a theme or discard them first.
A theme you make keeps the corners and label style of the theme you started from.

## Where it is kept

The theme is stored by the app (in `settings.json` in the [data folder](files.md)), so the desktop window and every
browser showing the app use the same one. A browser also remembers the last theme it showed, so the page doesn't
flash in the default colours while it connects. The splash screen always uses the default theme.
