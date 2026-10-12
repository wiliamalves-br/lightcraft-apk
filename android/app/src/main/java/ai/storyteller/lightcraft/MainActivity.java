package ai.storyteller.lightcraft;

import android.Manifest;
import android.app.AlertDialog;
import android.app.NativeActivity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.provider.Settings;
import android.view.View;

/**
 * LightCraft's activity: Android's NativeActivity running the Rust app (liblightcraft_android.so,
 * see android.app.lib_name in the manifest). The Java side does only what native code can't do
 * simply: ask once for access to the photos on the device, and keep the app full screen.
 */
public class MainActivity extends NativeActivity {
    private boolean askedForStorage = false;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        hideSystemBars();
    }

    @Override
    protected void onResume() {
        super.onResume();
        hideSystemBars();
        askForStorage();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            hideSystemBars();
        }
    }

    @SuppressWarnings("deprecation")
    private void hideSystemBars() {
        getWindow().getDecorView().setSystemUiVisibility(
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                        | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                        | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_FULLSCREEN);
    }

    /** Import reads photos by path from any folder, export writes them anywhere: that takes "All files access". */
    private void askForStorage() {
        if (askedForStorage) {
            return;
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            if (Environment.isExternalStorageManager()) {
                return;
            }
            askedForStorage = true;
            new AlertDialog.Builder(this)
                    .setTitle("Acesso às suas fotos")
                    .setMessage("Para importar fotos de qualquer pasta (Câmera, WhatsApp, cartão SD…) e exportar "
                            + "para a galeria, o LightCraft precisa do \"Acesso a todos os arquivos\". "
                            + "Na próxima tela, ative a opção para o LightCraft e volte para o app.")
                    .setPositiveButton("Permitir", (dialog, which) -> openAllFilesSettings())
                    .setNegativeButton("Agora não", null)
                    .show();
        } else if (checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED) {
            askedForStorage = true;
            requestPermissions(new String[] {
                    Manifest.permission.READ_EXTERNAL_STORAGE, Manifest.permission.WRITE_EXTERNAL_STORAGE
            }, 1);
        }
    }

    private void openAllFilesSettings() {
        try {
            Intent intent = new Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION);
            intent.setData(Uri.parse("package:" + getPackageName()));
            startActivity(intent);
        } catch (Exception e) {
            startActivity(new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION));
        }
    }
}
