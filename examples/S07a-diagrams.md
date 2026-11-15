---
title: Diagrams
description: Mermaid diagrams in code blocks, with themes and failures.
tags: [diagrams, mermaid]
---

# Diagrams

Each section says what to look for; anything else is a bug. Themes and
colourless output: [README.md](README.md):

```sh
kiri examples/S07a-diagrams.md
```

## A diagram in a code block

A Mermaid diagram in a code block is drawn as an image in kitty, as GitHub
draws it, and stays code elsewhere and with `--no-images`. This flowchart
runs top to bottom, with a box titled "Saunan lämmitys" around four steps.
From its diamond, "ei" leads to "Lisää puita" and back up, and "kyllä"
leaves the box for the last one. The boxes are lavender on a light page
and dark grey on a dark one. The letters ä and ö show, the labels sit on
their arrows, and no text is cut off.

```mermaid
flowchart TD
    Alku([Lauantai-ilta]) --> Puut
    subgraph Sauna [Saunan lämmitys]
        Puut[Kanna puut sisään] --> Sytytä[Sytytä tuli]
        Sytytä --> Lämmin{Onko jo 80 astetta?}
        Lämmin -->|ei| Lisää[Lisää puita]
        Lisää --> Lämmin
    end
    Lämmin -->|kyllä| Löyly([Löylyyn])
```

## A diagram that chooses its own theme

kiri draws a diagram for the page it lies on: Mermaid's `default` theme on
a light page and its `dark` theme on a dark one. A diagram can choose a
theme with a directive on its first line and keeps it; one made for the
other page lies on a panel of its own background, a little wider than the
diagram. The first diagram below chooses `dark` and the second `default`,
so on either page one of them is on a panel:

```mermaid
%%{init: {"theme": "dark"}}%%
flowchart LR
    Chosen[Chose the dark theme] --> Page[Light page: on a panel]
```

```mermaid
%%{init: {"theme": "default"}}%%
flowchart LR
    Chosen[Chose the default theme] --> Page[Dark page: on a panel]
```

On a light page the first lies on a dark grey panel, on a dark page the
second on a white one, and the other has the page behind it:

```sh
kiri --light examples/S07a-diagrams.md
kiri --dark examples/S07a-diagrams.md
```

`--diagram-panel off`, `KIRI_DIAGRAM_BACKGROUND_PANEL=off` or
`background_panel = false` in the `[diagrams]` table leaves the panels out,
and the diagram for the other page is then hard to see. See
[Diagrams](../docs/diagrams.md).

## A diagram that fails to draw

A diagram that cannot be drawn stays code. This one cannot, because its
last arrow leads nowhere:

```mermaid
flowchart LR
    Start --> Middle
    Middle -->
```

Below the code, in red, kiri says why, in lines that start with a cross
and "mermaid diagram could not be drawn". They are no wider than the code's
box, so a long reason wraps. A warning on standard error says the same and
names the diagram by its line. By default you get both; `--render-errors`
chooses one:

```sh
kiri --render-errors inline examples/S07a-diagrams.md
kiri --render-errors stderr examples/S07a-diagrams.md
kiri --mode kitty --render-errors stderr examples/S07a-diagrams.md >/dev/null
```

With `inline` there is no warning; with `stderr` the diagram is code alone.
The last command shows only the warning, `kiri: mermaid diagram on line
…: …`.

## One diagram of each type

One small diagram of each common Mermaid type, to compare with GitHub's
drawing of the same blocks. Each is also a file of its own in
[mermaid/](mermaid/), which kiri shows fitted to the window, and as source
with `--format text`:

```sh
kiri examples/mermaid/flowchart.mmd
kiri --format text examples/mermaid/flowchart.mmd
```

[S07b-flowchart.mmd](S07b-flowchart.mmd) is a flowchart of its own here,
so that `kiri examples/S*` shows it among the other files. From its
diamond, "no" leads to "Wait a while" and back up, and "yes" leads to a
database-shaped box at the end:

```sh
kiri examples/S07b-flowchart.mmd
```

View the document on `--light` and `--dark` too. No text is cut off, and
the letters ä and ö show.

### Flowchart

A push's way through continuous integration, with a subgraph, labelled
arrows, a dotted arrow back and a database-shaped node:
[flowchart.mmd](mermaid/flowchart.mmd).

```mermaid
flowchart LR
    Push([Push to a branch]) --> Build[Build]
    subgraph CI [Continuous integration]
        Build --> Tests[Run the tests]
        Tests --> Green{All green?}
    end
    Green -->|yes| Review[Code review]
    Green -->|no| Fix[Fix and push again]
    Fix -.-> Push
    Review --> Merge[(Merge into main)]
```

### Sequence diagram

A login with numbered steps, an activation bar, notes and an alternative
for a wrong password: [sequence.mmd](mermaid/sequence.mmd).

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant App as Mobile app
    participant API
    participant DB as Database
    User->>App: Tap "Log in"
    App->>API: POST /login
    activate API
    API->>DB: Look up the user
    DB-->>API: User and password hash
    Note over API: Check the password
    alt password is right
        API-->>App: 200 OK and a token
        App-->>User: Show the home screen
    else password is wrong
        API-->>App: 401 Unauthorized
        App-->>User: Show an error
    end
    deactivate API
    Note right of App: The token is kept<br/>until the user logs out
```

### Class diagram

A small library in Finnish, with an interface, multiplicities and labelled
associations: [class.mmd](mermaid/class.mmd).

```mermaid
classDiagram
    direction LR
    class Kirja {
        +String nimi
        +String kirjoittaja
        +int vuosi
        +lainaa(Asiakas) Laina
    }
    class Asiakas {
        +String nimi
        +String kortinNumero
        +lainat() List~Laina~
    }
    class Laina {
        +Date eräpäivä
        +palauta()
        +onMyöhässä() bool
    }
    class Aineisto {
        <<interface>>
        +hylly() String
    }
    Aineisto <|.. Kirja
    Asiakas "1" --> "0..*" Laina : has
    Laina "0..*" --> "1" Kirja : of
```

### State diagram

A pull request, with a nested state and a note:
[state.mmd](mermaid/state.mmd).

```mermaid
stateDiagram-v2
    [*] --> Draft
    Draft --> InReview : open a pull request
    InReview --> Draft : changes requested
    InReview --> Approved : approve
    Approved --> Merged : merge
    Merged --> [*]
    state InReview {
        [*] --> Waiting
        Waiting --> Reviewing : reviewer starts
        Reviewing --> Waiting : new push
    }
    note right of Approved
        Needs one approval
        and green tests
    end note
```

### Entity relationship diagram

A web shop's tables, columns, keys and crow's-foot relations:
[er.mmd](mermaid/er.mmd).

```mermaid
erDiagram
    CUSTOMER ||--o{ ORDER : places
    ORDER ||--|{ ORDER_LINE : contains
    PRODUCT ||--o{ ORDER_LINE : "appears in"
    CUSTOMER {
        int id PK
        string name
        string email UK
    }
    ORDER {
        int id PK
        int customer_id FK
        date ordered_on
    }
    ORDER_LINE {
        int order_id FK
        int product_id FK
        int quantity
    }
    PRODUCT {
        int id PK
        string name
        decimal price
    }
```

### Gantt chart

A cottage renovation in Finnish, with finished and active tasks,
dependencies and a milestone: [gantt.mmd](mermaid/gantt.mmd).

```mermaid
gantt
    title Kesämökin remontti
    dateFormat YYYY-MM-DD
    axisFormat %d.%m.
    section Suunnittelu
        Mittaukset          :done,    mitat, 2026-05-04, 3d
        Tarvikkeet tilattu  :done,    tilaus, after mitat, 4d
    section Työt
        Vanha terassi pois  :active,  purku, 2026-05-14, 3d
        Uusi terassi        :         terassi, after purku, 6d
        Maalaus             :         maali, after terassi, 4d
    section Valmis
        Juhannus            :milestone, 2026-06-19, 0d
```

### Pie chart

Shares of a whole, with the values in the legend: [pie.mmd](mermaid/pie.mmd).

```mermaid
pie showData
    title Where a working day goes
    "Writing code" : 35
    "Reviews" : 15
    "Meetings" : 20
    "Reading docs" : 10
    "Coffee" : 20
```

### Mind map

Household chores in Finnish, branching from a centre:
[mindmap.mmd](mermaid/mindmap.mmd).

```mermaid
mindmap
  root((Kotitalous))
    Ruoka
      Kauppalista
      Viikon ruokalista
    Siivous
      Imurointi
      Pyykit
    Talous
      Laskut
      Säästötili
    Piha
      Nurmikko
      Lumityöt
```

### Timeline

Events by period, in sections: [timeline.mmd](mermaid/timeline.mmd).

```mermaid
timeline
    title A small project's first year
    section Start
        January : Idea and first sketches
        February : Prototype : First users try it
    section Growth
        May : Version 1.0
        August : Plugins : Translations
    section Upkeep
        November : Bug fixes : Faster start-up
```

### Git graph

Commits, branches, a merge and a tag: [gitgraph.mmd](mermaid/gitgraph.mmd).

```mermaid
gitGraph
    commit id: "init"
    commit id: "readme"
    branch feature/search
    checkout feature/search
    commit id: "search box"
    commit id: "highlight"
    checkout main
    commit id: "fix typo"
    merge feature/search
    branch release
    checkout release
    commit id: "v1.0" tag: "v1.0"
    checkout main
    commit id: "next"
```
