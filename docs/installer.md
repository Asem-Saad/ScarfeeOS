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
| Full | Laptops with a supported GPU | Larger local model on the GPU |
| Lite | Laptops without a suitable GPU | Smaller local model on the CPU |

- Exact models and hardware limits are set once benchmarks finish (see [models.md](models.md)).
- If the chosen tier is more than the hardware can handle, the installer warns the user but does not block the choice.
- Both tiers share the code-based features, such as date handling, so they do not depend on the model.

## Later steps (planned)
1. Language
2. Disk and account setup
3. Hardware check and AI tier selection
4. Optional: connect Claude (API key), or stay local-only
5. Finish
