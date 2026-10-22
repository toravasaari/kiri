# Diagrams

A document for checking kiri's diagrams by eye in a real kitty window. Each
section says what you should see there with the default theme; anything else
is a bug. How to view it with the themes and without colours is in
[README.md](README.md):

```sh
cargo run -- examples/S07a-diagrams.md
```

## A diagram in a code block

A Mermaid diagram in a code block is drawn as an image in kitty, as GitHub
draws it, and stays code elsewhere and with `--no-images`. This flowchart
runs top to bottom: a rounded box, then a box titled "Saunan lämmitys"
around four steps. From its diamond, the arrow "ei" leads to "Lisää
puita" and another arrow back up, and the arrow "kyllä" leaves the box
for a rounded box at the end. On a light page the boxes are lavender with
dark text; on a dark page they are dark grey with light text. The letters
ä and ö show, the labels sit on their arrows, and no text is cut off.

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

## One diagram of each type

One small diagram of each common Mermaid type, as kiri draws them. On
GitHub the same blocks are drawn as diagrams too, so the two can be
compared side by side. Each diagram is also a file of its own in
[mermaid/](mermaid/), which kiri shows on its own, fitted to the window,
and shows as source with `--format text`:

```sh
kiri examples/mermaid/flowchart.mmd
kiri --format text examples/mermaid/flowchart.mmd
```

View the document on the light and dark paper themes too. On a light
page the diagrams use Mermaid's `default` theme, lavender boxes with
dark text; on a dark page its `dark` theme, dark grey boxes with light
text. No text is cut off, and the letters ä and ö show.

### Flowchart

Boxes joined by arrows, here a push's way through continuous
integration, with a subgraph, labelled arrows, a dotted arrow back to the
start and a database-shaped node. Source:
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

Messages between participants over time: a login with numbered steps, an
activation bar, notes and an alternative for a wrong password. Source:
[sequence.mmd](mermaid/sequence.mmd).

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

Classes with fields and methods and how they relate: a small library in
Finnish, with an interface, multiplicities and labelled associations.
Source: [class.mmd](mermaid/class.mmd).

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

States and the events that move between them: a pull request, with a
nested state and a note. Source: [state.mmd](mermaid/state.mmd).

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

Tables of a web shop, their columns and keys, and crow's-foot relations
between them. Source: [er.mmd](mermaid/er.mmd).

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

Tasks on a calendar: a summer cottage renovation in Finnish, with finished
and active tasks, dependencies and a milestone. Source:
[gantt.mmd](mermaid/gantt.mmd).

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

Shares of a whole, with the values shown in the legend. Source:
[pie.mmd](mermaid/pie.mmd).

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

Ideas branching from a centre: household chores in Finnish. Source:
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

Events by period, grouped into sections. Source:
[timeline.mmd](mermaid/timeline.mmd).

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

Commits, branches, a merge and a tag. Source:
[gitgraph.mmd](mermaid/gitgraph.mmd).

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
