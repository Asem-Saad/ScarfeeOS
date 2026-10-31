# Installer

## Language selection comes first
The very first screen of the installer asks which language to install and use. Installation cannot continue until one is chosen.

```
Select your language / اختر لغتك

   [ English ]        [ العربية ]
```

- The screen is shown bilingually, since the user has not chosen yet.
- After the choice, the rest of the installer uses that language, with RTL layout for Arabic.
- The choice sets: system locale, keyboard layout (with the option to add the other language's layout), default AI conversation and speech language, calendar and numeral defaults.
- Both languages remain installed; the user can switch later in Settings without a restart.

## AI tier selection
The installer checks the hardware, recommends a tier and lets the user choose. A GPU is not required to install ScarfeeOS.

| Tier | For | Runs |
|------|-----|------|
| Dual | Laptops with a GPU of about 8 GB or more | Two local models on the GPU: Qwen for commands, Jais for Arabic writing and explanation |
| Full | Laptops with a supported GPU | Larger local model on the GPU |
| Lite | Laptops without a suitable GPU | Smaller local model on the CPU, or on the GPU from about 4 GB |

- Dual uses Qwen3-4B with Jais-3-3.7B (about 7.7 GB measured), or Qwen3-8B with Jais-3-3.7B (about 9.8 GB measured) from about 12 GB.
- Full uses Qwen3-8B (5.8 GB measured). Lite uses Qwen3-4B (3.7 GB measured on a GPU). The Lite model is not final: the CPU benchmark is still pending.
- Exact models and hardware limits are set once benchmarks finish (see [models.md](models.md)).
- If the chosen tier is more than the hardware can handle, the installer warns the user but does not block the choice.
- All tiers share the code-based features, such as date handling, so they do not depend on the model.

## Steps
The installer shows these screens in this order:
1. Language
2. AI tier selection, after the hardware check
3. Disk and account setup (a marked placeholder for now: nothing is written to a disk yet)

Planned after these:
4. Optional: connect Claude (API key), or stay local-only
5. Finish
