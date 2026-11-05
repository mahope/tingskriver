# Sprog: hovedsprog, ekstra talesprog og referatsprog

Issue #209. Koden ligger i `crates/tingskriver-core/src/meeting_language.rs`.

| Indstilling | Standard | Betydning |
| --- | --- | --- |
| Hovedsprog | `da` | Bruges til referat og er altid med som transskriptionssprog. |
| Ekstra talesprog | ingen | Sprog, der også tales (fx `en`). Giver transskriptionen kontekst. |
| Skriv altid referat på hovedsproget | til | Til: referatet er på hovedsproget. Fra: referatet følger det mest talte af de valgte sprog, hvis det kan registreres, ellers hovedsproget. |

Valgbare sprog: dansk, engelsk, svensk, norsk (`no`), tysk, fransk, spansk,
hollandsk og finsk.

## Gemt og bagudkompatibelt

Indstillingerne ligger i `meeting_language.json` i appens datamappe. Findes
filen ikke, bruges standarderne. De svarer til adfærden før indstillingen
fandtes (dansk transskription, dansk referat). Manglende felter får standard.
En ulæselig fil giver en fejl på indstillingssiden og stopper nye referater i
stedet for stiltiende at skifte sprog.

## Hvad hver del får

* **Syv.ai og andre OpenAI-kompatible udbydere:** `language` er hovedsproget.
  Med ekstra talesprog står der desuden en sætning først i `prompt`, fx
  "Samtalen foregår på dansk og engelsk." Uden ekstra sprog er kaldet uændret.
  Det er en prompt og ikke et særligt felt, fordi det virker hos alle
  udbydere, der forstår `prompt`.
* **Lokal Whisper og Parakeet:** kun hovedsproget. Whisper tager ét sprog ad
  gangen, og Parakeet har ingen sprogvalgsindstilling. De ekstra talesprog
  bruges ikke lokalt.
* **Køen:** `JobSpec.extra_languages` (tom for jobs gemt, før feltet blev
  indført) sendes med i hver forespørgsel.
* **Referatet:** `MeetingLanguages::summary_language` vælger sproget. Sproget
  valgt på selve mødet vinder altid. Referatprompten får sproget som
  eksplicit instruktion ("into Danish"), og titlen følger referatet.

## Eksisterende møder

Sprogindstillingerne ændrer ikke gemte transskripter eller køens versioner. Et
nyt hovedsprog oversætter ikke et gammelt transskript. Vil man have et referat
på det nye sprog, laver man referatet om. Indstillingssiden siger det.
