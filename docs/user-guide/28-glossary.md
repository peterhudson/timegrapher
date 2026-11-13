# 28. Glossary

The words this guide uses, each with where to read more.

| Word | What it means | More |
| --- | --- | --- |
| **1H30, 4H30, 7H30, 10H30** | The four 45° stops between the crown positions, named by the hour at the top: 1:30 Up and so on. For a stand that stops every 45°. | Chapters 4, 5 |
| **AGC** (automatic gain control) | A microphone or sound card turning its own level up and down. Keep it off: it changes the loudness of the sounds the app times. | Chapter 3 |
| **Allan deviation** | How the scatter of an average of the readings changes as the average gets longer. | Chapter 14, M9 |
| **Amplitude** | How far the balance turns from rest to the end of its swing, in degrees. Worked out from the time from unlock to drop and the lift angle. | Chapter 1, M4 |
| **Autocorrelation** | How much each reading follows the one before. Readings that follow each other make a plain ± too small. | Chapter 13, M7 |
| **Average Over** | The seconds each live reading covers, 10 s unless set. | Chapter 5, M6 |
| **Beat** | One swing of the balance from one end to the other. Two beats make one oscillation. | Chapter 1 |
| **Beat error** | Half the difference between the times of the Tick and the Tock halves of the swing, in ms. Shown as a size, without a sign. | Chapter 1, M5 |
| **Beat rate (bph)** | Beats per hour the movement is made to make, such as 28,800 bph. | Chapter 1 |
| **Bench** | The app's working view: the cards, the Paper Strip and the panes. | Chapter 4 |
| **Calibre** | The movement's maker and number. It sets the beat rate, the lift angle and the wheels. | Chapters 5, 24 |
| **Change point** | A moment where a reading's level moves and stays moved. | Chapter 15, M9 |
| **Clipping** | Sound too loud for the input, cut flat at its top. It spoils the timing of the sounds. | Chapter 3 |
| **Clock correction** | The sound card's clock is off by a steady few parts per million (ppm), which moves every rate. Measured with Quartz Correction, from USB lines, or by `timegrapher clock`, and taken out. | Chapter 3, M3 |
| **CSV** (comma-separated values) | A plain table file any spreadsheet opens. | Chapters 20, 21 |
| **CUSUM** (cumulative sum) | A running sum of each reading less the mean. A steady slope in it marks a level that changed. | Chapter 15, M9 |
| **Complication Test** | A complication, such as a chronograph, off, on, then off again in one position, and how much it changes the amplitude, rate and beat error (On less Off). | Chapter 20c |
| **Cut** | A place no reading may cross: a pause, a jump, a position change, a mask, or over 2 s with no beat. | M6 |
| **Cycle** | A rise and fall a reading repeats, often at the period of a wheel. | Chapters 11, 12, M10 |
| **D, DVH, X** | A positions test's largest difference in rate, its vertical less horizontal, and its average. | Chapter 20, M8 |
| **dBFS** (decibels relative to full scale) | Loudness against the loudest the input can record, which is 0 dBFS. | Chapter 3 |
| **Drop** | The third sound of a beat: an escape-wheel tooth landing on the other pallet stone. Usually the loudest. | Chapter 1 |
| **DU, DD, CU, CD, CL, CR** | The six positions: dial up, dial down, and crown up, down, left and right. Also written CH, CB, 3H, 6H, 9H, 12H. | Chapters 5, 20 |
| **Dynamic Poise Test** | The rate in the eight upright positions, 45° apart, fitted with the wave it makes once round. | Chapter 20b, M13 |
| **FFT** (Fast Fourier Transform) | The standard way to split a series into the regular swings it holds. The FFT view gives the size of each period's swing over the whole take. | Chapter 16, M11 |
| **FLAC** (Free Lossless Audio Codec) | A sound file format that keeps every sample in less space than WAV. | Chapter 23 |
| **Full wind, 24 hours after** | A positions test run just after winding, and again a day later, to see how the rate moves as the mainspring lets down. | Chapter 20 |
| **Ie, Im** | The change in mean rate, and the largest change in one position (crown right left out), from full wind to 24 hours after. | Chapter 20, M8 |
| **Impulse** | The second sound of a beat: the escape wheel pushing the pallet stone. Usually the quietest. | Chapter 1 |
| **Isochronism** | Whether the rate stays the same as the amplitude falls. | Chapter 19, M12 |
| **Jitter** | How far single beats land from the steady line the rate gives. | Chapter 5 |
| **JSON** (JavaScript Object Notation) | A text format programs read; the command line prints it with `--json`. | Chapter 25 |
| **Lift angle** | The small arc the balance passes through from unlock to drop, set by the movement's design. Every amplitude is in proportion to it. | Chapter 1, M4 |
| **Ljung–Box** | A test of whether a series of readings follows itself more than chance would. | Chapter 13, M9 |
| **Mask** | A stretch you mark to leave out. Masked time is cut, never filled in. | Chapter 6 |
| **MCP** | The Model Context Protocol, how a chat app such as Claude Desktop uses the timegrapher. | Chapter 26 |
| **NTP** (Network Time Protocol) | How a computer keeps its clock right from internet time servers. | Chapter 3, M3 |
| **Paper Strip** | The beats drawn as dots, as a paper timegrapher draws them: a slope is the rate, the gap between the two lines the beat error. | Chapter 6 |
| **Period** | The time a cycle takes to repeat. Steadiness names one after the wheel that turns once in it. | Chapter 11 |
| **Position Brush** | Marks a stretch of a take as one position, after the fact. | Chapter 6 |
| **Positions Test** | The rate, amplitude and beat error in each of six positions, guided or marked. | Chapter 20 |
| **Poise, heavy spot** | A balance out of poise is heavier on one side, its heavy spot. Upright, it moves the rate by the hour at the top: least at 219.5° amplitude, and the other way above it. | Chapter 20b, M13 |
| **Power Reserve Test** | How long a full wind lasts, recorded until the watch stops. | Chapter 21 |
| **ppm** (parts per million) | A clock's error as a fraction: 1 ppm is about 0.086 s a day. | Chapter 3, M3 |
| **Quick Listen** | Listening with no watch picked. A take kept goes to Unsorted. | Chapter 2 |
| **Rate (s/d)** | How many seconds a day the watch would gain (+) or lose (−) running as it is now. | Chapter 1, M2 |
| **Session** | What is on the Bench, from one **New Session** to the next. A take's `session.toml` lists its stretches for the command line's `session`. | Chapters 23, 25 |
| **Settle** | The seconds after each change of position left out while the balance steadies. | Chapter 20, M6 |
| **Steadiness** | The panes that ask how the readings move over a take, and how sure each answer is. | Chapter 9 |
| **STFT map** (Short-Time Fourier Transform) | The FFT of each 20 minutes of readings in turn: each period's size through the take, window by window. | Chapter 17, M11 |
| **Take** | One recording of a watch, kept as a folder: its sound, its clock and its settings. | Chapter 23 |
| **Tick and Tock** | The app's names for the two kinds of beat, blue and orange. They don't say which pallet stone is which. | Chapter 1 |
| **Two states** | Readings flipping between two levels, such as the balance swinging at two amplitudes in turn. | Chapter 10 |
| **Unlock** | The first sound of a beat: the impulse pin striking the fork. Amplitude and beat error are timed from it. | Chapter 1 |
| **Unsorted** | Where Quick Listen takes wait until filed under a watch. | Chapter 22 |
| **Watches folder** | The folder that holds every watch and take, `Timegrapher Data` in your home folder unless you chose another. | Chapters 22 to 24 |
| **WAV** (Waveform Audio File Format) | A sound file format that keeps every sample as it is, uncompressed. | Chapter 23 |
| **Wavelet map** | Where and when each swing is in the take, sharper in time than the STFT map. A wavelet is a short burst of a wave, stretched to each period and slid along the readings. | Chapter 18, M11 |

---

Previous: [Problems and Reports](27-problems-and-reports.md) ·
[Contents](README.md)
