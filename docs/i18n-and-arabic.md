# English and Arabic (i18n) design rules

English (`en`) and Arabic (`ar`) are both main languages. Every user-facing feature must work fully in both.

## Rules
1. **No hard-coded strings.** All text goes through Fluent (`locales/<lang>/*.ftl`).
2. **RTL is mirrored, not patched.** Layout uses logical start/end, never left/right. The whole shell flips for Arabic.
3. **Runtime switch.** Language changes apply without a restart.
4. **Bidirectional text** (Arabic mixed with English, numbers, URLs) must render correctly. Use HarfBuzz shaping and the Unicode bidi algorithm.
5. **Fonts:** Noto Naskh/Kufi Arabic, IBM Plex Arabic.
6. **Numerals:** support Western and Eastern Arabic numerals as a user setting.
7. **Calendar:** Gregorian and Hijri; configurable week start and weekend (e.g. Sunday-Thursday work week).
8. **Every UI change is checked in both directions** before merging.

## AI and speech
- Model selection is benchmarked on Arabic as well as English; a model that fails Arabic is not eligible as a default.
- Support Modern Standard Arabic first, then Gulf, Egyptian and Levantine dialects.
- Support code-switching (Arabic and English in one sentence).
- Speech-to-text, text-to-speech (with diacritization) and the wake word are evaluated on Arabic audio under `core/eval/`.
