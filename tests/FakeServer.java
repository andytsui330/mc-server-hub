// 假的 Minecraft 伺服器：用真的 java.exe 跑，只測平台的開關機，不碰任何真世界。
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;

public class FakeServer {
    static void varint(ByteArrayOutputStream b, int n) {
        while (true) {
            int x = n & 0x7F; n >>>= 7;
            b.write(n != 0 ? x | 0x80 : x);
            if (n == 0) return;
        }
    }
    static int rv(InputStream in) throws IOException {
        int n = 0, s = 0;
        while (true) {
            int x = in.read();
            if (x < 0) throw new EOFException();
            n |= (x & 0x7F) << s;
            if ((x & 0x80) == 0) return n;
            s += 7;
        }
    }
    static String difficulty = "Normal";
    static void out(String s) { System.out.println(s); System.out.flush(); }

    public static void main(String[] a) throws Exception {
        int port = 25565;
        for (String l : Files.readAllLines(Path.of("server.properties"))) if (l.startsWith("server-port=")) port = Integer.parseInt(l.substring(12).trim());
        out("[12:00:00] [main/INFO]: Loading fake server");
        out("[12:00:01] [Server thread/WARN]: 測試用警告：中文顯示正常");
        Thread.sleep(1000);
        ServerSocket srv = new ServerSocket(port);
        Thread t = new Thread(() -> {
            while (true) try (Socket c = srv.accept()) {
                InputStream in = c.getInputStream();
                in.readNBytes(rv(in)); rv(in); in.read();
                byte[] body = "{\"version\":{\"name\":\"fake\",\"protocol\":767},\"players\":{\"max\":15,\"online\":1,\"sample\":[{\"name\":\"Tester\",\"id\":\"0\"}]},\"description\":\"fake\"}".getBytes(StandardCharsets.UTF_8);
                ByteArrayOutputStream p = new ByteArrayOutputStream(), f = new ByteArrayOutputStream();
                varint(p, 0); varint(p, body.length); p.write(body);
                varint(f, p.size()); f.write(p.toByteArray());
                c.getOutputStream().write(f.toByteArray());
            } catch (IOException e) { if (srv.isClosed()) return; }
        });
        t.setDaemon(true); t.start();
        out("[12:00:03] [Server thread/INFO]: Done (2.0s)! For help, type \"help\"");
        BufferedReader r = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        for (String cmd; (cmd = r.readLine()) != null; ) {
            cmd = cmd.trim();
            if (cmd.equals("stop")) {
                out("[12:00:09] [Server thread/INFO]: Stopping the server");
                Thread.sleep(1000);
                out("[12:00:11] [Server thread/INFO]: ThreadedAnvilChunkStorage: All dimensions are saved");
                System.exit(0);
            } else if (cmd.equals("difficulty")) out("[12:00:05] [Server thread/INFO]: The difficulty is " + difficulty);
            else if (cmd.startsWith("difficulty ")) {
                String d = cmd.substring(11);
                difficulty = d.substring(0, 1).toUpperCase() + d.substring(1);
                out("[12:00:05] [Server thread/INFO]: The difficulty has been set to " + difficulty);
            } else if (cmd.equals("list")) out("[12:00:05] [Server thread/INFO]: There are 1 of a max of 15 players online: Tester");
            else out("[12:00:05] [Server thread/INFO]: 收到指令：" + cmd);
        }
    }
}
