# Function parity: LibreOffice Calc and ferrosheet

Companion to the [feature comparison](feature-parity.md). It lists every spreadsheet
function and whether ferrosheet can calculate it.

**Today (phase 1)** ferrosheet calculates nothing itself. It shows the result that Excel or
Calc stored in the file, for every function, and the formula in the formula bar. A changed
input does not update anything, because nothing can be edited yet.

**Phase 2** adds [IronCalc](https://github.com/ironcalc/IronCalc) as the calculation engine.
Then the functions below are recalculated live. A function IronCalc lacks keeps showing the
stored result and gets `#NAME?` once its cell is recalculated.

In short: of the 522 functions Calc offers, IronCalc covers 493. The 29 it lacks are
mostly LibreOffice extras; four of them exist in Excel too. IronCalc has 4 functions Calc lacks.

| Category | IronCalc | also in Calc | only in IronCalc |
| --- | ---: | ---: | ---: |
| Math and trigonometry | 82 | 81 | 1 |
| Statistical | 105 | 105 | 0 |
| Financial | 55 | 55 | 0 |
| Date and time | 25 | 25 | 0 |
| Text | 46 | 44 | 2 |
| Logical | 19 | 19 | 0 |
| Information | 22 | 22 | 0 |
| Lookup and reference | 34 | 33 | 1 |
| Database | 17 | 17 | 0 |
| Engineering | 55 | 55 | 0 |
| Compatibility (pre-2010 names) | 37 | 37 | 0 |
| **Total** | **497** | **493** | **4** |

Sources: function list of LibreOffice core (`formula/inc/core_resource.hrc` plus the
Analysis and Pricing add-ins), IronCalc `base/src/functions/mod.rs`, both as of 2026-11.

## Functions by category

Every function here is calculated by IronCalc in phase 2. Marked ones:

- ⁺ only in IronCalc, Calc has no such function
- name (Calc: X) same function, different name in Calc

### Math and trigonometry

`ABS`, `ACOS`, `ACOSH`, `ASIN`, `ASINH`, `ATAN`, `ATAN2`, `ATANH`, `CHOOSE`, `COLUMN`, `COLUMNS`, `COS`, `COSH`, `LOG`, `LOG10`, `LN`, `MAX`, `MIN`, `PI`, `POWER`, `PRODUCT`, `RAND`, `RANDBETWEEN`, `ROUND`, `ROUNDDOWN`, `ROUNDUP`, `SIN`, `SINH`, `SQRT`, `SQRTPI`, `SUM`, `SUMIF`, `SUMIFS`, `SUMX2MY2`, `SUMX2PY2`, `SUMXMY2`, `TAN`, `TANH`, `ACOT`, `ACOTH`, `COT`, `COTH`, `CSC`, `CSCH`, `SEC`, `SECH`, `EXP`, `FACT`, `FACTDOUBLE`, `SIGN`, `RADIANS`, `DEGREES`, `INT`, `EVEN`, `ODD`, `CEILING`, `CEILING.MATH`, `CEILING.PRECISE`, `FLOOR`, `FLOOR.MATH`, `FLOOR.PRECISE`, `ISO.CEILING`, `MOD`, `QUOTIENT`, `MROUND`, `TRUNC`, `GCD`, `LCM`, `BASE`, `DECIMAL`, `ROMAN`, `ARABIC`, `COMBIN`, `COMBINA`, `SUMSQ`, `MDETERM`, `MINVERSE`, `MUNIT`, `MULTINOMIAL`, `SERIESSUM`, `SUMPRODUCT`, `PERCENTOF`⁺

### Statistical

`AVERAGE`, `AVERAGEA`, `AVERAGEIF`, `AVERAGEIFS`, `COUNT`, `COUNTA`, `COUNTBLANK`, `COUNTIF`, `COUNTIFS`, `MAXIFS`, `MINIFS`, `GEOMEAN`, `FREQUENCY`, `GROWTH`, `AVEDEV`, `BETA.DIST`, `BETA.INV`, `BINOM.DIST`, `BINOM.DIST.RANGE` (Calc: `B`), `BINOM.INV`, `CHISQ.DIST`, `CHISQ.DIST.RT`, `CHISQ.INV`, `CHISQ.INV.RT`, `CHISQ.TEST`, `CONFIDENCE.NORM`, `CONFIDENCE.T`, `COVARIANCE.P`, `COVARIANCE.S`, `DEVSQ`, `EXPON.DIST`, `F.DIST`, `F.DIST.RT`, `F.INV`, `F.INV.RT`, `F.TEST`, `FISHER`, `FISHERINV`, `FORECAST`, `FORECAST.LINEAR`, `FORECAST.ETS` (Calc: `FORECAST.ETS.ADD`), `FORECAST.ETS.CONFINT` (Calc: `FORECAST.ETS.PI.ADD`), `FORECAST.ETS.SEASONALITY`, `FORECAST.ETS.STAT` (Calc: `FORECAST.ETS.STAT.ADD`), `GAMMA`, `GAMMA.DIST`, `GAMMA.INV`, `GAMMALN`, `GAMMALN.PRECISE`, `GAUSS`, `HARMEAN`, `HYPGEOM.DIST`, `KURT`, `LARGE`, `LINEST`, `LOGEST`, `LOGNORM.DIST`, `LOGNORM.INV`, `MAXA`, `MEDIAN`, `MINA`, `MODE.MULT`, `MODESINGL` (Calc: `MODE.SNGL`), `NEGBINOM.DIST`, `NORM.DIST`, `NORM.INV`, `NORM.S.DIST`, `NORM.S.INV`, `PEARSON`, `PERCENTILE.EXC`, `PERCENTILE.INC`, `PERCENTRANK.EXC`, `PERCENTRANK.INC`, `PERMUT`, `PERMUTATIONA`, `PHI`, `POISSON.DIST`, `PROB`, `QUARTILE.EXC`, `QUARTILE.INC`, `RANK.AVG`, `RANK.EQ`, `SKEW`, `SKEW.P`, `SMALL`, `STANDARDIZE`, `STDEV`, `STDEV.P`, `STDEV.S`, `STDEVA`, `STDEVPA`, `T.DIST`, `T.DIST.2T`, `T.DIST.RT`, `T.INV`, `T.INV.2T`, `T.TEST`, `TREND`, `TRIMMEAN`, `VAR.P`, `VAR.S`, `VARPA`, `VARA`, `WEIBULL.DIST`, `Z.TEST`

### Financial

`ACCRINT`, `ACCRINTM`, `DISC`, `FVSCHEDULE`, `INTRATE`, `PRICEDISC`, `PRICEMAT`, `RECEIVED`, `YIELDDISC`, `YIELDMAT`, `CUMIPMT`, `CUMPRINC`, `DB`, `DDB`, `DOLLARDE`, `DOLLARFR`, `EFFECT`, `FV`, `IPMT`, `IRR`, `ISPMT`, `MIRR`, `NOMINAL`, `NPER`, `NPV`, `PDURATION`, `PMT`, `PPMT`, `PV`, `RATE`, `RRI`, `SLN`, `SYD`, `TBILLEQ`, `TBILLPRICE`, `TBILLYIELD`, `XIRR`, `XNPV`, `DURATION`, `MDURATION`, `PRICE`, `YIELD`, `ODDFPRICE`, `ODDFYIELD`, `ODDLPRICE`, `ODDLYIELD`, `COUPDAYBS`, `COUPDAYS`, `COUPDAYSNC`, `COUPNCD`, `COUPNUM`, `COUPPCD`, `AMORDEGRC`, `AMORLINC`, `VDB`

### Date and time

`DATE`, `DATEDIF`, `DATEVALUE`, `DAY`, `EDATE`, `EOMONTH`, `MONTH`, `TIME`, `TIMEVALUE`, `HOUR`, `MINUTE`, `SECOND`, `NOW`, `TODAY`, `YEAR`, `NETWORKDAYS`, `NETWORKDAYS.INTL`, `DAYS`, `DAYS360`, `WEEKDAY`, `WEEKNUM`, `WORKDAY`, `WORKDAY.INTL`, `YEARFRAC`, `ISOWEEKNUM`

### Text

`CONCAT`, `TEXTSPLIT`, `CONCATENATE`, `EXACT`, `FIND`, `LEFT`, `LEN`, `LOWER`, `MID`, `REPT`, `RIGHT`, `SEARCH`, `SUBSTITUTE`, `REGEXEXTRACT` (Calc: `REGEX`), `REGEXREPLACE` (Calc: `REGEX`), `REGEXTEST` (Calc: `REGEX`), `T`, `TEXT`, `TEXTAFTER`, `TEXTBEFORE`, `TEXTJOIN`, `TRIM`, `UNICODE`, `UNICHAR`, `CHAR`, `CLEAN`, `CODE`, `ASC`, `ARRAYTOTEXT`, `DOLLAR`, `ENCODEURL`, `ECMACEILING`⁺, `FINDB`, `FIXED`, `LEFTB`, `LENB`, `MIDB`, `NUMBERVALUE`, `PROPER`, `REPLACE`, `REPLACEB`, `RIGHTB`, `SEARCHB`, `UPPER`, `VALUE`, `VALUETOTEXT`⁺

### Logical

`AND`, `FALSE`, `IF`, `IFERROR`, `IFNA`, `IFS`, `LAMBDA`, `LET`, `NOT`, `OR`, `SWITCH`, `TRUE`, `XOR`, `BYCOL`, `BYROW`, `MAP`, `REDUCE`, `SCAN`, `MAKEARRAY`

### Information

`ERROR.TYPE`, `FORMULATEXT` (Calc: `FORMULA`), `ISBLANK`, `ISERR`, `ISERROR`, `ISEVEN`, `ISFORMULA`, `ISLOGICAL`, `ISNA`, `ISNONTEXT`, `ISNUMBER`, `ISODD`, `ISOMITTED`, `ISREF`, `ISTEXT`, `NA`, `SHEET`, `TYPE`, `SHEETS`, `N`, `CELL`, `INFO`

### Lookup and reference

`ADDRESS`, `AREAS`, `CHOOSECOLS`, `CHOOSEROWS`, `EXPAND`, `FILTER`, `HLOOKUP`, `HSTACK`, `HYPERLINK`, `INDEX`, `INDIRECT`, `LOOKUP`, `MATCH`, `OFFSET`, `ROW`, `ROWS`, `SORT`, `SORTBY`, `UNIQUE`, `VLOOKUP`, `VSTACK`, `WRAPCOLS`, `WRAPROWS`, `XLOOKUP`, `XMATCH`, `TRIMRANGE`⁺, `TAKE`, `DROP`, `TOCOL`, `TOROW`, `TRANSPOSE`, `MMULT`, `SEQUENCE`, `RANDARRAY`

### Database

`DAVERAGE`, `DCOUNT`, `DGET`, `DMAX`, `DMIN`, `DSUM`, `DCOUNTA`, `DPRODUCT`, `DSTDEV`, `DVAR`, `DVARP`, `DSTDEVP`, `CORREL`, `RSQ`, `INTERCEPT`, `SLOPE`, `STEYX`

### Engineering

`BESSELI`, `BESSELJ`, `BESSELK`, `BESSELY`, `ERF`, `ERFC`, `ERFC.PRECISE`, `ERF.PRECISE`, `BIN2DEC`, `BIN2HEX`, `BIN2OCT`, `DEC2BIN`, `DEC2HEX`, `DEC2OCT`, `HEX2BIN`, `HEX2DEC`, `HEX2OCT`, `OCT2BIN`, `OCT2DEC`, `OCT2HEX`, `BITAND`, `BITLSHIFT`, `BITOR`, `BITRSHIFT`, `BITXOR`, `COMPLEX`, `IMABS`, `IMAGINARY`, `IMARGUMENT`, `IMCONJUGATE`, `IMCOS`, `IMCOSH`, `IMCOT`, `IMCSC`, `IMCSCH`, `IMDIV`, `IMEXP`, `IMLN`, `IMLOG10`, `IMLOG2`, `IMPOWER`, `IMPRODUCT`, `IMREAL`, `IMSEC`, `IMSECH`, `IMSIN`, `IMSINH`, `IMSQRT`, `IMSUB`, `IMSUM`, `IMTAN`, `CONVERT`, `DELTA`, `GESTEP`, `SUBTOTAL`

### Compatibility (pre-2010 names)

`BETADIST`, `HYPGEOMDIST`, `LOGNORMDIST`, `NEGBINOMDIST`, `NORMSDIST`, `TDIST`, `BETAINV`, `BINOMDIST`, `CHIDIST`, `CHIINV`, `CHITEST`, `CONFIDENCE`, `COVAR`, `CRITBINOM`, `EXPONDIST`, `FDIST`, `FINV`, `FTEST`, `GAMMADIST`, `GAMMAINV`, `LOGINV`, `MODE`, `NORMDIST`, `NORMINV`, `NORMSINV`, `PERCENTILE`, `PERCENTRANK`, `POISSON`, `QUARTILE`, `RANK`, `STDEVP`, `TINV`, `TTEST`, `VAR`, `VARP`, `WEIBULL`, `ZTEST`

## Only in Calc

Functions Calc has and IronCalc lacks. A cell using one of them keeps showing the result
stored in the file. Four also exist in Excel and are rare in everyday workbooks.

| Function | What it does |
| --- | --- |
| `AGGREGATE` | SUBTOTAL that can skip errors and hidden rows (Excel has it too) |
| `BAHTTEXT` | Thai number words |
| `COLOR` | RGB value |
| `CURRENT` | value of the current formula step |
| `DDE` | live link to another program |
| `EASTERSUNDAY` | date of Easter Sunday |
| `EUROCONVERT` | old national currencies to euro |
| `FILTERXML` | reads values from XML (Excel has it too) |
| `FORECAST.ETS.MULT` | forecast with multiplicative seasonality |
| `FORECAST.ETS.PI.MULT` | same, prediction interval |
| `FORECAST.ETS.STAT.MULT` | same, statistics |
| `GETPIVOTDATA` | reads a pivot table (Excel has it too) |
| `GOALSEEK` | goal seek as a formula |
| `JIS` | Japanese full-width characters |
| `MULTIRANGE` | several ranges as one argument |
| `MVALUE` | matrix value |
| `NEG` | negation |
| `OPT_BARRIER` | option pricing |
| `OPT_PROB_HIT` | option pricing |
| `OPT_PROB_INMONEY` | option pricing |
| `OPT_TOUCH` | option pricing |
| `RAND.NV` | random number that does not recalculate |
| `RANDBETWEEN.NV` | same |
| `RAWSUBTRACT` | subtraction without rounding |
| `ROUNDSIG` | round to significant digits |
| `STYLE` | sets a cell style |
| `TABLE` | multiple operations (what-if table) |
| `WEBSERVICE` | loads a URL (Excel has it too) |
| `ZGZ` | same as RRI |
