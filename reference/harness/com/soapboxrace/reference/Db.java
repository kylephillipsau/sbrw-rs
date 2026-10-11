package com.soapboxrace.reference;

import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.sql.SQLException;
import java.sql.Statement;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Resets the runtime tables between scenarios and snapshots them afterwards.
 *
 * <p>Tables that hold rows after the initial load of Schema.sql + Data.sql are
 * static game data and are left alone, except {@code parameter}, which scenarios
 * may override and which is restored each time. Every other table is runtime
 * state: truncated, with its starting AUTO_INCREMENT restored.
 */
public final class Db implements AutoCloseable {
    private static final Pattern AUTO_INCREMENT = Pattern.compile("\\) ENGINE=\\w+ AUTO_INCREMENT=(\\d+)");
    private static final DateTimeFormatter DATE_TIME = DateTimeFormatter.ofPattern("yyyy-MM-dd HH:mm:ss");

    private final Connection conn;
    private final List<String> runtimeTables = new ArrayList<>();
    private final Map<String, Long> autoIncrements = new TreeMap<>();
    private final Map<String, String> parameters = new LinkedHashMap<>();

    public Db(String url, String user, String password) throws SQLException {
        conn = DriverManager.getConnection(url, user, password);
        try (Statement st = conn.createStatement();
             ResultSet rs = st.executeQuery("SELECT TABLE_NAME FROM information_schema.TABLES "
                     + "WHERE TABLE_SCHEMA = DATABASE() ORDER BY TABLE_NAME")) {
            List<String> tables = new ArrayList<>();
            while (rs.next()) tables.add(rs.getString(1));
            for (String t : tables) {
                if (count(t) == 0) runtimeTables.add(t);
                Long ai = autoIncrement(t);
                if (ai != null) autoIncrements.put(t, ai);
            }
        }
        try (Statement st = conn.createStatement(); ResultSet rs = st.executeQuery("SELECT NAME, VALUE FROM parameter")) {
            while (rs.next()) parameters.put(rs.getString(1), rs.getString(2));
        }
    }

    /**
     * The table's next AUTO_INCREMENT value as SHOW CREATE TABLE reports it, or null
     * when it has none. information_schema.TABLES.AUTO_INCREMENT can't be used: MySQL 8
     * reports it from cached statistics, and gave null for persona's starting 100.
     */
    private Long autoIncrement(String table) throws SQLException {
        try (Statement st = conn.createStatement(); ResultSet rs = st.executeQuery("SHOW CREATE TABLE `" + table + "`")) {
            rs.next();
            Matcher m = AUTO_INCREMENT.matcher(rs.getString(2));
            return m.find() ? Long.valueOf(m.group(1)) : null;
        }
    }

    private long count(String table) throws SQLException {
        try (Statement st = conn.createStatement(); ResultSet rs = st.executeQuery("SELECT COUNT(*) FROM `" + table + "`")) {
            rs.next();
            return rs.getLong(1);
        }
    }

    public List<String> runtimeTables() {
        return runtimeTables;
    }

    public void reset(Map<String, String> parameterOverrides) throws SQLException {
        try (Statement st = conn.createStatement()) {
            st.execute("SET FOREIGN_KEY_CHECKS = 0");
            for (String t : runtimeTables) {
                st.execute("TRUNCATE TABLE `" + t + "`");
                Long ai = autoIncrements.get(t);
                if (ai != null && ai > 1) st.execute("ALTER TABLE `" + t + "` AUTO_INCREMENT = " + ai);
            }
            st.execute("SET FOREIGN_KEY_CHECKS = 1");
            st.execute("DELETE FROM parameter");
        }
        Map<String, String> params = new LinkedHashMap<>(parameters);
        if (parameterOverrides != null) params.putAll(parameterOverrides);
        try (PreparedStatement ps = conn.prepareStatement("INSERT INTO parameter (NAME, VALUE) VALUES (?, ?)")) {
            for (Map.Entry<String, String> e : params.entrySet()) {
                ps.setString(1, e.getKey());
                ps.setString(2, e.getValue());
                ps.addBatch();
            }
            ps.executeBatch();
        }
    }

    /** Every runtime table that has rows, ordered by primary key. */
    public Map<String, List<Map<String, Object>>> snapshot() throws SQLException {
        Map<String, List<Map<String, Object>>> out = new TreeMap<>();
        for (String t : runtimeTables) {
            List<Map<String, Object>> rows = new ArrayList<>();
            try (Statement st = conn.createStatement(); ResultSet rs = st.executeQuery("SELECT * FROM `" + t + "` ORDER BY 1")) {
                ResultSetMetaData md = rs.getMetaData();
                while (rs.next()) {
                    Map<String, Object> row = new LinkedHashMap<>();
                    for (int i = 1; i <= md.getColumnCount(); i++) {
                        row.put(md.getColumnLabel(i), normalize(rs.getObject(i)));
                    }
                    rows.add(row);
                }
            }
            if (!rows.isEmpty()) out.put(t, rows);
        }
        return out;
    }

    /** Canonical JSON values, so the Rust side can produce identical snapshots from SQLite. */
    static Object normalize(Object v) {
        if (v == null || v instanceof String || v instanceof Boolean || v instanceof Number) {
            return v;
        }
        if (v instanceof LocalDateTime) {
            return formatDateTime((LocalDateTime) v);
        }
        if (v instanceof java.sql.Timestamp) {
            return formatDateTime(((java.sql.Timestamp) v).toLocalDateTime());
        }
        if (v instanceof LocalDate || v instanceof java.sql.Date) {
            return v.toString();
        }
        if (v instanceof byte[]) {
            StringBuilder sb = new StringBuilder("0x");
            for (byte b : (byte[]) v) sb.append(String.format("%02x", b));
            return sb.toString();
        }
        return v.toString();
    }

    private static String formatDateTime(LocalDateTime t) {
        String s = t.format(DATE_TIME);
        int micros = t.getNano() / 1000;
        return micros == 0 ? s : s + String.format(".%06d", micros);
    }

    @Override
    public void close() throws SQLException {
        conn.close();
    }
}
