// 假的 NeoForge 安裝器：--installServer 時照真安裝器的樣子產生 run.bat、user_jvm_args.txt 與 libraries/.../win_args.txt。
// 版本號從自己的 jar 檔名取（neoforge-<版本>-installer.jar）；資料夾名含 fail 就以代碼 1 結束，用來測失敗路徑。
import java.nio.charset.StandardCharsets;
import java.nio.file.*;

public class FakeInstaller {
    public static void main(String[] a) throws Exception {
        Path self = Path.of(FakeInstaller.class.getProtectionDomain().getCodeSource().getLocation().toURI());
        String name = self.getFileName().toString();
        String v = name.replaceAll("^neoforge-(.+)-installer\\.jar$", "$1");
        Path cwd = Path.of("").toAbsolutePath();
        Path log = cwd.resolve(name + ".log");
        System.out.println("Fake installer for NeoForge " + v);
        System.out.println("Extracting main jar...");
        Files.writeString(log, "fake installer log\n", StandardCharsets.UTF_8);
        if (a.length == 0 || !a[0].equals("--installServer") || cwd.getFileName().toString().contains("fail")) {
            System.out.println("There was an error during installation");
            Files.writeString(log, "There was an error during installation\n", StandardCharsets.UTF_8, StandardOpenOption.APPEND);
            System.exit(1);
        }
        String lib = "libraries/net/neoforged/neoforge/" + v;
        Files.createDirectories(cwd.resolve(lib));
        Files.copy(self, cwd.resolve(lib + "/fake-server.jar"), StandardCopyOption.REPLACE_EXISTING);
        Files.writeString(cwd.resolve(lib + "/win_args.txt"), "-cp " + lib + "/fake-server.jar FakeServer\n");
        Files.writeString(cwd.resolve("user_jvm_args.txt"), "# Xmx and Xms set the maximum and minimum RAM usage\n");
        Files.writeString(cwd.resolve("run.bat"), "@echo off\r\nREM Add custom JVM arguments to the user_jvm_args.txt\r\n"
            + "java @user_jvm_args.txt @" + lib + "/win_args.txt %*\r\npause\r\n");
        System.out.println("The server installed successfully");
    }
}
