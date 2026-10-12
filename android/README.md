# LightCraft para Android

O mesmo LightCraft do desktop — a mesma interface egui e o mesmo motor de revelação em Rust — rodando
num APK Android (arm64). Não é uma versão reduzida: biblioteca, catálogo, todos os painéis de
revelação, máscaras, presets e perfis, cópia/colagem de ajustes, sincronização e exportação são o
código do desktop.

## O que muda no celular

| Desktop | Android |
|---|---|
| Janelas de arquivo do sistema | Seletor de arquivos próprio, feito para toque: marque fotos de várias pastas, **Selecionar todos**, ou **Importar esta pasta inteira** (com subpastas) |
| Clique direito | Toque longo |
| Ctrl/Shift + clique para selecionar várias | Botão **•••** ▸ *Ctrl: tocar soma à seleção* / *Shift: selecionar intervalo* (ficam ligados até você desligar) |
| Atalhos de teclado | Botão **•••**: Importar, Exportar, Importar presets, Colar ajustes, Selecionar tudo, Desfazer/Refazer, Configurações, zoom da interface (A−/A+) |
| Esc | Botão Voltar |

O botão **•••** fica no canto inferior esquerdo (*Lado →* muda para a direita).

### Fluxo em massa (importar → preset → exportar)

1. **••• ▸ Importar pasta…** (ou *Importar fotos…* e marque várias) — escolha, por exemplo, `DCIM/Camera`.
   Na revisão da importação dá para escolher um preset para já aplicar em todas na entrada.
2. Na grade, **••• ▸ Selecionar tudo** (ou ligue *Ctrl* e toque nas fotos que quiser).
3. Aplique um preset: com várias selecionadas, toque no preset (ele vale para todas as selecionadas) —
   ou edite uma foto, *Copiar ajustes*, e **••• ▸ Colar ajustes…** com as outras selecionadas.
4. **••• ▸ Exportar…** — a pasta padrão é `Imagens/LightCraft Exports` (aparece na galeria). Toque em
   *Escolher…* para outra pasta.

Presets do Lightroom (`.xmp`, `.lrtemplate`, `.zip`), `.lcpreset`, Luminar e LUTs `.cube` entram por
**••• ▸ Importar presets…**.

### Permissão

Na primeira abertura o app pede **Acesso a todos os arquivos** — é o que deixa importar de qualquer
pasta (Câmera, WhatsApp, cartão SD) e exportar para a galeria. Se negar, ative depois em
Configurações ▸ Apps ▸ LightCraft ▸ Permissões.

### Onde ficam as coisas

- Biblioteca e configurações: armazenamento privado do app (`/data/user/0/ai.storyteller.lightcraft/files`).
  Para usar uma biblioteca no armazenamento compartilhado, *Configurações ▸ Abrir biblioteca…*.
- Log: `…/files/config/logs/lightcraft.log` (útil para relatar problemas).
- HEIC/HEIF (formato padrão de muitas câmeras Android) está incluído nesta build.

## Gerar o APK

### Pelo GitHub (sem instalar nada)

1. Faça um fork deste repositório.
2. Aba **Actions** ▸ **Android APK** ▸ **Run workflow**.
3. Ao terminar (a primeira compilação leva bastante tempo; as próximas usam cache), baixe o artefato
   **lightcraft-android-apk** e instale `lightcraft-android.apk` no celular.

Assinatura: sem configurar nada, o APK é assinado com uma chave de depuração descartável — instala
normalmente, mas cada build nova pede para desinstalar a anterior. Para atualizar por cima, crie uma
chave (`keytool -genkeypair -v -keystore release.jks -keyalg RSA -keysize 2048 -validity 10000 -alias lightcraft`)
e cadastre em *Settings ▸ Secrets ▸ Actions*: `ANDROID_KEYSTORE_BASE64` (`base64 -w0 release.jks`),
`ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`.

### Na sua máquina

Precisa de Rust, Android SDK + NDK, JDK 17 e Gradle 8.9+:

```sh
export ANDROID_HOME=~/Android/Sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/<versão>
bash android/build-apk.sh        # → target/lightcraft-android.apk
```

## Como é feito

- `apps/lightcraft-android` — o app Rust (`cdylib`, `android_main`): abre a biblioteca, entrega à
  interface os serviços da plataforma (o seletor de arquivos para todos os diálogos) e acrescenta o
  que falta numa tela de toque. Desenha com OpenGL ES (eframe `glow`); o processamento na GPU usa
  Vulkan (wgpu) quando o aparelho permite, senão a CPU.
- `android/` — o projeto Gradle: uma `NativeActivity` em Java que só pede a permissão de arquivos e
  mantém a tela cheia.
- `.github/workflows/android.yml` — compila tudo e publica o APK.

Limitações conhecidas: a interface é a do desktop (pensada para paisagem; o app abre em paisagem);
"Mostrar no gerenciador de arquivos", "Editar em outro editor" e links externos não existem no
Android.
