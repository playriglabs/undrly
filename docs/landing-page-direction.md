# Undrly — Landing Page Direction

Status: **Disetujui untuk implementasi; diperbarui mengikuti arahan dark mode dan Inter Tight.**  
Tanggal: 30 September 2026  
Scope: **6 section utama, tanpa footer**, dark mode, responsive.

### Arahan terbaru dari review

- Section Developers & Agents dihapus. CTA developer menuju quickstart/API contracts langsung; halaman kini memiliki enam section.

- Hero mengikuti komposisi screenshot Linear yang diberikan: headline besar rata kiri, subcopy di bawah, kemudian frame preview dashboard lebar.
- Area preview dashboard **kosong dulu**. Konten dashboard akan ditambahkan nanti; tidak membuat UI dashboard atau data dummy sekarang.
- Ilustrasi fitur mengadopsi gaya isometric line art bergerak dari screenshot Linear pertama, diterjemahkan ke dark mode.
- Implementasi disetujui di `landing/`: SvelteKit, Tailwind, GSAP, clsx, dan ts-pattern.
- Arahan terbaru mengganti HBSet/Urbanist menjadi **Inter Tight**, heading Regular 400 dengan tracking moderat.
- Satu section wajib berisi trio ornamen isometrik bergerak seperti referensi Linear.
- Revisi Collect: ilustrasi tumpukan diganti tiga source record yang turun ke capture frame.
- Preview lokal dahulu; deployment belum dilakukan.

## 1. Tujuan dan pemahaman produk

Undrly adalah financial data infrastructure: mengumpulkan data pasar yang terfragmentasi, menormalisasi, mengagregasi observasi yang sesuai, lalu menyajikannya melalui satu API yang konsisten.

Positioning utama tetap mengikuti repo:

> One normalized API across every market.

Identitas instrumen, relationship graph, dan provenance menjadi alasan kenapa hasilnya bisa dipahami dan ditelusuri. Ketiganya mendukung pesan utama; pengunjung tidak perlu memahami ontology sebelum mengerti manfaat Undrly.

**Audiens utama:** developer aplikasi finansial, pembangun produk lintas aset, dan developer AI agent yang membutuhkan data finansial terstruktur.

**Tujuan landing:** pengunjung memahami apa yang Undrly satukan, melihat perbedaannya dari feed harga biasa, lalu membuka dokumentasi atau setup lokal.

### Fitur yang menjadi dasar konten

| Kapabilitas | Yang tersedia menurut repo | Implikasi untuk landing |
| --- | --- | --- |
| Normalized quotes | Equities, crypto spot, FX, commodities, perpetuals | Tampilkan lima kategori dengan satu pola akses |
| Aggregation | Misalnya mid-price BTC/USD dari Kraken dan Coinbase yang memenuhi aturan freshness | Jelaskan bahwa agregasi mengikuti semantik data, bukan mencampur semua harga |
| Identity resolution | Query melalui simbol, nama, pair, identifier, venue symbol, atau canonical ID | Perlihatkan bagaimana query dipetakan ke instrumen yang tepat |
| Explain & graph | Bukti resolution, hubungan instrumen, listing, deployment, dan market | Hubungan jelas tanpa menyamakan objek yang berbeda |
| Market context | Candles, reference history, market status, derivatives, calendar | Pendalaman produk setelah nilai dasar dipahami |
| Provenance | Raw source record disimpan; quote memiliki unit, basis, timestamp, freshness | Data dapat ditelusuri, bukan sekadar angka |
| MCP | Sembilan read-only tools memakai logika yang sama dengan REST API | Jalur integrasi untuk aplikasi dan AI agent |

### Batas klaim

- README membedakan hackathon V1 dan pengembangan lokal V1.1–V1.8. Landing boleh menjelaskan kapabilitas yang ada, tetapi tidak menyebutnya layanan publik production-ready.
- Tidak mengklaim uptime, latency, jumlah pelanggan, volume transaksi, jumlah feed aktif, atau partnership tanpa bukti.
- Tidak menawarkan streaming, SDK, signup, billing, atau hosted remote MCP seolah sudah tersedia.
- Data provider saat ini untuk local/private demo; contoh visual menggunakan data sintetis berlabel **Illustrative example**, bukan feed live yang dipublikasikan.
- Nama exchange/provider menjadi konteks sumber atau venue, bukan baris “Trusted by”.
- Token, underlying, deployment, USD, USDC, dan USDT tetap objek berbeda. Diagram tidak boleh menyiratkan semuanya interchangeable.

## 2. Arah kreatif

### Konsep: “Clarity beneath every market”

Pasar terlihat kompleks di permukaan. Undrly memperlihatkan struktur yang rapi di bawahnya. Narasi visual bergerak dari beberapa sumber, masuk ke satu layer normalisasi, lalu keluar menjadi interface yang konsisten.

Nuansanya editorial, presisi, tenang, dan premium. Daya tarik berasal dari tipografi besar, komposisi asimetris, ruang kosong, dan satu motif visual yang kuat.

### Peran referensi

| Referensi | Arah yang diambil | Adaptasi untuk Undrly |
| --- | --- | --- |
| [Pyth Network](https://www.pyth.network/) | Framing market-data infrastructure dan narasi lintas aset | Headline langsung, cakupan pasar jelas, alur menuju developer |
| [Linear](https://linear.app/) | Referensi yang diminta untuk spacing, layout, dan ornament illustration | Grid disiplin, pemisahan konten lega, ilustrasi terarah yang menjelaskan sistem |
| [Supabase](https://supabase.com/) | Referensi yang diminta untuk minimalisme konten | Copy singkat, manfaat konkret, contoh developer mudah ditemukan |

Referensi tersebut menjadi arah, bukan template yang disalin. Detail visual perlu diinspeksi saat implementasi; riset awal sudah mencakup isi halaman referensi.

### Sistem visual awal

| Elemen | Rencana |
| --- | --- |
| Background utama | Near-black `#090B0A` |
| Surface | Charcoal `#101310`, green-black `#10160F` |
| Teks utama | Off-white `#ECEEEB` |
| Teks sekunder | Abu netral `#969D96` |
| Accent | Muted sage `#B6C8A6`, pemakaian terbatas |
| Border | `#242925`, garis tipis |
| Heading | **Inter Tight Regular 400**, di-host lokal |
| Body, navigasi, label, tombol | **Inter Tight**, regular/medium |
| Kode | System monospace agar identifier dan sintaks terbaca |
| Layout | Maksimum konten sekitar 1200–1280 px, desktop grid 12 kolom |
| Spacing | Section desktop 120–160 px; mobile 64–88 px |
| Sudut | Umumnya 8–16 px; pill hanya untuk label atau kategori |
| Efek | Shadow tipis, kedalaman lembut, highlight material yang terkontrol |

Dark mode dan Inter Tight mengikuti arahan terbaru pengguna. Font di-host lokal melalui Fontsource. Aset HBSet asli tetap tersimpan di brands. Wordmark memakai teks “undrly”.

Heading desktop sekitar 80–104 px untuk hero dan 48–64 px untuk section. Mobile sekitar 44–56 px dan 32–40 px, menyesuaikan metrik Inter Tight. Body 17–20 px dengan line-height lapang. Heading memakai weight 400, tanpa synthetic bold.

## 3. Struktur halaman

Navigasi di luar hitungan section: wordmark kiri; **Markets / How it works / Developers**; CTA **Get started**. Navigasi sticky ringan dengan background gelap; mobile memakai menu sederhana yang dapat dibuka melalui keyboard.

Semua copy publik di bawah diusulkan dalam **bahasa Inggris**. Penjelasan review tetap bahasa Indonesia.

| # | Section | Peran |
| --- | --- | --- |
| 1 | Hero | Menjelaskan produk dan membangun kesan pertama |
| 2 | Market coverage | Menunjukkan luas kategori dengan akses yang konsisten |
| 3 | Normalization pipeline | Menjelaskan bagaimana fragmentasi diselesaikan |
| 4 | Financial identity | Memperlihatkan struktur di balik simbol |
| 5 | Data provenance | Menjelaskan arti dan asal setiap quote |
| 6 | Developers & agents | Membuktikan kemudahan integrasi REST dan MCP |
| 7 | Closing CTA | Mengarahkan ke langkah berikutnya |

Halaman berakhir pada section CTA; footer dihapus sesuai arahan terbaru.

## 4. Detail tujuh section

### 01 — Hero

**Eyebrow:** tidak ditampilkan pada hero agar komposisi mengikuti referensi dan tetap bersih.

**Headline:**

> Every market.  
> One clear interface.

**Body:**

> Bring equities, crypto, FX, commodities, and perpetuals into one normalized API. Clear identities. Traceable data. Built for your next application.

**CTA:** satu text link `Start building ↗` → section developer, di sisi kanan baris subcopy pada desktop. Mobile ditempatkan di bawah subcopy. CTA utama `Get started` tetap tersedia di navigasi.

**Layout:** mengikuti screenshot hero Linear yang diberikan pengguna. Headline dua baris besar rata kiri, memiliki ruang kosong di sisi kanan. Subcopy berada di bawah headline. Setelah jeda vertikal sekitar 64–88 px, satu frame preview dashboard memenuhi lebar kontainer. Hero bukan komposisi dua kolom teks dan ilustrasi.

**Preview dashboard:** surface kosong charcoal, border abu tipis, radius sekitar 12–16 px, dan shadow sangat lembut. Rasio awal sekitar 16:9, menyesuaikan viewport. Tanpa sidebar, chart, angka, skeleton, loading indicator, atau tulisan “Coming soon”. Ini ruang untuk aset preview dashboard nanti, bukan permintaan membangun dashboard sekarang. Frame tetap dark mode meskipun referensi berwarna gelap.

**Motion:** entrance tipografi dan frame singkat serta tenang. Frame kosong tidak dianimasikan seolah sedang memuat data. Ilustrasi aliran sumber dipindahkan ke section penjelasan produk.

**Sketsa komposisi desktop:**

```text
undrly                  Markets   How it works   Developers   Get started

Every market.
One clear interface.

Bring equities, crypto, FX, commodities,                    Start building ↗
and perpetuals into one normalized API. ...

┌─────────────────────────────────────────────────────────────────────────┐
│                                                                         │
│                                                                         │
│                                                                         │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 02 — Many markets. A common language.

**Eyebrow:** `MARKET COVERAGE`

**Headline:**

> Many markets.  
> A common language.

**Body:**

> Move across asset classes without rebuilding your data model for each one.

**Layout:** satu panel lebar dengan lima tab pasar, bukan lima kartu generik yang identik. Pergantian tab memperlihatkan query, jenis harga, dan konteksnya.

| Tab | Contoh query | Copy pendek |
| --- | --- | --- |
| Equities | `NVDA` | Venue quotes with market-session context. |
| Crypto | `BTC/USD` | One aggregate from eligible venue observations. |
| FX | `EUR/USD` | Venue prices and reference rates, clearly distinguished. |
| Commodities | `XAU/USD` | Reference prices with explicit units. |
| Perpetuals | `BTC-PERP` | Mark prices with funding and contract context. |

**Visual:** path request konsisten `GET /v1/quote/{query}` dengan informasi semantik yang berubah. Tidak perlu harga bergerak atau grafik trading dekoratif.

**Interaksi:** tab dapat dipakai via keyboard; mobile dapat scroll horizontal dengan indikator. Contoh statis diberi label, tidak memanggil provider.

### 03 — From fragmented to connected.

**Eyebrow:** `HOW UNDRLY WORKS`

**Headline:**

> From fragmented  
> to connected.

**Body:**

> Different sources, formats, and conventions. One consistent path from raw observations to usable market data.

**Empat langkah:**

1. **Collect** — Preserve the original source response.
2. **Normalize** — Align instruments, units, venues, and timestamps.
3. **Aggregate** — Combine eligible observations under explicit rules.
4. **Serve** — Read consistent results through one API.

**Layout:** diagram horizontal besar di desktop, vertikal di mobile. Step menjadi label pada diagram, bukan deretan kartu terpisah.

**Visual:** jalur-jalur masuk yang semula berbeda menjadi rapi setelah melewati layer Undrly. Diagram harus tetap menjelaskan proses saat motion dimatikan.

**Detail opsional:** kalimat pendek di bawah: `Your request reads from Undrly’s storage, without waiting on upstream providers.` Hindari angka latency tanpa benchmark.

### 04 — Beyond the ticker.

**Eyebrow:** `FINANCIAL IDENTITY`

**Headline:**

> Beyond the ticker.  
> Into the underlying.

**Body:**

> Resolve the instrument behind a query. Explore its markets and relationships, with the evidence behind each match.

**Tiga label fitur:** `Resolve precisely` / `Explore relationships` / `Explain every match`.

**Layout:** teks ringkas di kiri, diagram hubungan di kanan. Ini section paling editorial setelah hero.

**Diagram utama:** Bitcoin sebagai underlying; BTC/USD sebagai market; BTC perpetual sebagai instrumen berbeda dengan hubungan `DERIVES_FROM`; Hyperliquid sebagai venue. Label eksplisit membedakan price denomination USDT dari margin dan settlement USDC.

Diagram menunjukkan relasi yang dikenal repo. Jangan menggambar direct equivalence antara spot dan perpetual atau menambahkan hubungan tokenized equities demi mempercantik visual.

**Interaksi:** memilih node menampilkan satu penjelasan pendek; informasi inti tetap terlihat tanpa hover.

### 05 — Know what’s behind the number.

**Eyebrow:** `DATA WITH CONTEXT`

**Headline:**

> Know what’s behind  
> the number.

**Body:**

> A price is only useful when you know what it means. Keep its unit, basis, timestamp, and freshness in view—and trace the observations behind it.

**Layout:** latar green-black dengan satu komposisi quote anatomy. Fokus pada field yang diberi anotasi, bukan dashboard.

**Tiga pesan pendukung:**

- **Explicit units** — Know what the price is denominated in.
- **Visible freshness** — Understand when the observation was recorded.
- **Traceable observations** — Follow a canonical quote back to its source observations.

**Visual:** response ilustratif dengan `basis`, `unit`, `asOf`, dan `freshness`; callout tipis ke masing-masing field. Struktur response final harus dicocokkan dengan kontrak Zod sebelum ditulis sebagai JSON valid.

Canonical quote tidak menyebut semua provider secara langsung; asal observasi dijelaskan melalui endpoint `/v1/quotes/{query}`. Jangan menambahkan `source` fiktif ke canonical response.

### 06 — Built for your stack. Ready for your agents.

**Eyebrow:** `DEVELOPERS & AGENTS`

**Headline:**

> Built for your stack.  
> Ready for your agents.

**Body:**

> Query through REST or connect through MCP. Your application and your agent work with the same underlying model.

**Layout:** teks dan CTA di kiri, code panel charcoal di kanan. Tab **REST API** dan **MCP tools**. Palet tetap konsisten dengan dark mode halaman.

**REST example:**

```bash
curl "http://localhost:8787/v1/quote/BTC/USD"
```

Caption: `After running Undrly locally.`

**MCP panel:** tampilkan contoh pemanggilan tool, dilabeli sebagai ilustrasi tool call, bukan konfigurasi client siap tempel:

```json
{
  "name": "get_quote",
  "arguments": { "query": "BTC/USD" }
}
```

Copy pendukung: `Nine read-only tools for discovery, identity, quotes, history, and derivatives.`

**CTA:** `Read the quickstart` dan `Explore MCP` menuju README dan dokumentasi MCP repo. Tautan branch aktif harus diverifikasi sebelum implementasi; tidak menganggap domain docs atau API publik sudah berjalan.

**Interaksi:** tab dan copy button dengan feedback `Copied`; kegagalan clipboard mendapat pesan yang jelas. Tidak ada tombol “Run” tanpa backend yang memang tersedia.

### 07 — One integration. More possibilities.

**Eyebrow:** `BUILD ON UNDRLY`

**Headline:**

> One integration.  
> More possibilities.

**Body:**

> Spend less time reconciling market data. Start building with a common foundation.

**CTA utama:** `Get started locally` → quickstart repo.  
**CTA sekunder:** `Explore the docs` → dokumentasi repo.

**Layout:** komposisi terpusat dengan ruang kosong besar. Motif layer dari section fitur muncul kembali sebagai ornamen kecil, memberikan penutup visual yang konsisten.

Tidak memasang waitlist, request access, atau form email sebelum ada tujuan dan proses yang nyata.

## 5. Penutup halaman

Footer dihapus sesuai arahan terbaru pengguna. Section 07 (Closing CTA) menjadi akhir halaman.

## 6. Motion, responsive, dan kualitas

### Referensi ilustrasi bergerak

Screenshot Linear pertama menjadi referensi spesifik: garis tipis, perspektif isometrik, susunan layer atau modul, detail understated, label figure kecil, serta ruang kosong luas. Adaptasi memakai garis abu di atas background gelap; jangan menyalin logo Linear.

Penerapan dalam tujuh section yang sudah ada, tanpa menambah section baru:

| Penempatan | Motif | Gerakan yang direncanakan |
| --- | --- | --- |
| Normalization pipeline | Tiga source record masuk ke capture frame isometrik | Record turun bergantian menuju frame penerimaan |
| Financial identity | Modul instrumen yang terhubung | Modul bergeser halus; hubungan yang relevan muncul berurutan |
| Data provenance | Lembaran record yang berurutan | Satu lembar maju sedikit untuk memperlihatkan jejak observasi |

Durasi loop awal sekitar 6–10 detik, dengan jeda istirahat. Gerakan kecil, tanpa putaran kamera dramatis atau bounce. Warna aksen hanya menandai bagian aktif. Anotasi penting tetap terbaca ketika ilustrasi diam.

“Motion” dapat berarti animasi secara umum atau nama library Motion. Screenshot statis tidak cukup untuk mengidentifikasi teknologi Linear. Pemilihan implementasi ilustrasi dan pengendali animasi ditentukan setelah review; tidak perlu mengasumsikan WebGL/3D runtime untuk tampilan isometrik ini.

### Perilaku umum

- Scroll native; tidak ada scroll hijacking atau mandatory loading intro.
- Reveal terbatas pada opacity dan translasi kecil sekitar 12–20 px.
- Hover tombol berupa perubahan warna dan gerakan arrow halus.
- Loop ilustrasi berhenti saat keluar viewport; hormati `prefers-reduced-motion` dengan komposisi statis.
- Mobile memakai satu kolom, diagram disusun ulang, bukan sekadar dikecilkan.
- Desktop hover memiliki padanan click/focus di touch dan keyboard.
- Heading, CTA, dan body bukan bagian dari gambar; tetap selectable dan accessible.
- Satu `h1`, hierarchy heading konsisten, focus visible, target sentuh nyaman, contrast diperiksa.
- Font di-host lokal bila memungkinkan; font-display swap dan aset visual dioptimalkan.
- Tidak menambahkan chart dekoratif, testimonial palsu, gradient berlebihan, atau kartu berulang tanpa fungsi.

## 7. Rencana implementasi setelah review

1. Finalisasi copy dan arah warna berdasarkan feedback dokumen ini.
2. Tetapkan folder landing yang terpisah dari API/data plane dan ikuti tooling repo yang relevan.
3. Tambahkan Inter Tight lokal, lalu bangun hero + navigasi sebagai dasar visual.
4. Implementasikan tujuh section, diagram, dan interaksi yang dijelaskan di atas.
5. Verifikasi isi terhadap kontrak aktual, destination link, responsiveness, keyboard, reduced motion, dan build.
6. Sajikan preview untuk review desain; keputusan deployment mengikuti scope yang disepakati berikutnya.

Implementasi berada di folder landing; platform hosting belum dipilih. Kode API dan data plane tidak diubah.

## 8. Poin review

- Apakah hero **“Every market. One clear interface.”** sudah pas, atau tagline asli harus menjadi headline utama?
- Dark mode + aksen sage sudah menjadi arahan terbaru.
- Apakah copy bahasa Inggris cocok untuk audiens landing?
- Apakah urutan tujuh section sudah menekankan produk sesuai prioritas?
- Apakah CTA ke quickstart lokal sesuai tahap produk saat ini?

## 9. Sumber produk

- [README](../README.md): positioning, coverage, API, versi lokal, dan batas penggunaan data.
- [AGENT.md](../AGENT.md): prinsip identitas produk dan arsitektur; instruksi landing pengguna menjadi scope baru untuk website marketing.
- [API contracts](contracts.md): acuan bentuk payload saat implementasi.
- [Cross-ecosystem identity](v1.4-cross-ecosystem-identity.md): model identitas dan hubungan; status historis perlu dibaca bersama versi terbaru.
- [MCP V1.8](v1.8-mcp.md): tools, provenance, semantic invariants, dan batas remote deployment.
- [HBSet](../brands/HBSetv0.96-Light.woff2): font brand awal; telah digantikan Inter Tight untuk landing sesuai arahan terbaru.
