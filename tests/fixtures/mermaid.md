# Mermaid diagrams

A flowchart with a subgraph and Finnish text:

```mermaid
flowchart TD
    Alku([Käyttäjä avaa dokumentin]) --> Jäsennys
    subgraph Käsittely [Käsittely kirissä]
        Jäsennys[Markdownin jäsennys] --> Malli[Dokumenttimalli]
        Malli --> Kaaviot{Onko kaavioita?}
        Kaaviot -->|kyllä| Piirto[Piirretään SVG:ksi]
        Kaaviot -->|ei| Asettelu
        Piirto --> Asettelu[Rivien asettelu]
    end
    Asettelu --> Näyttö[Näytetään päätteessä]
```

A sequence diagram with notes:

```mermaid
sequenceDiagram
    participant U as User
    participant K as kiri
    participant T as Terminal
    U->>K: open plan.md
    Note right of K: parse and lay out
    K->>T: query capabilities
    T-->>K: graphics, text sizing
    Note over K,T: detection happens once
    K->>T: escape sequences
    T-->>U: the page
```

A class diagram:

```mermaid
classDiagram
    class Document {
        +Vec~Block~ blocks
        +title() Option~String~
    }
    class Block {
        <<enumeration>>
        Heading
        Paragraph
        CodeBlock
    }
    class Renderer {
        <<interface>>
        +render(source, theme) Svg
    }
    Document "1" *-- "many" Block : contains
    Renderer <|.. Merman
```

A Gantt chart:

```mermaid
gantt
    title Diagrams in kiri
    dateFormat YYYY-MM-DD
    section Built in
        Image keys       :done,    keys, 2026-10-20, 1d
        merman           :active,  merman, after keys, 2d
    section External
        mmdc             :         mmdc, after merman, 2d
```

An entity relationship diagram:

```mermaid
erDiagram
    DOCUMENT ||--o{ BLOCK : contains
    BLOCK ||--o| IMAGE : shows
    IMAGE {
        string key
        int width
        int height
    }
```

A state diagram:

```mermaid
stateDiagram-v2
    [*] --> Parsed
    Parsed --> Loaded: images and diagrams
    Loaded --> Shown
    Shown --> Loaded: window resized
    Shown --> [*]
```

A diagram with a syntax error stays code and says why:

```mermaid
flowchart LR
    A[Unclosed label --> B
```

- A diagram in a list:

  ```mermaid
  flowchart LR
      Lista --> Kohta
  ```

> A diagram in a quote:
>
> ```mermaid
> flowchart LR
>     Lainaus --> Kaavio
> ```

A diagram that sets its own theme keeps it on any page:

```mermaid
%%{init: {"theme": "forest"}}%%
flowchart LR
    Metsä --> Teema
```
