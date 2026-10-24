# Third-party notices

This repository uses pinned, non-Mojang data and implementation references while
building the local protocol-1001 block catalog. Published source files stay
under the ignored `.local/assets/block-data/` directory, and the verified
download cache stays in the ignored sibling `.local/assets/block-data.downloads/`
directory. This notice is checked in and is not rewritten by the acquisition script.

## Android dedicated-server runtime

Experimental Android builds package [Box64](https://github.com/ptitSeb/box64), copyright (c) 2020 ptitSeb,
under the MIT license in `assets/licenses/Box64-MIT.txt`. Its source archive is pinned in
`packaging/android/runtime.json`.

These builds also package the GNU C Library with Android adaptations from
[glibc-packages](https://github.com/termux-pacman/glibc-packages). GNU C Library code is
copyright the Free Software Foundation and its other contributors; the Android adaptations
are copyright (c) 2023 Ivan Max under the MIT license in
`assets/licenses/Termux-glibc-MIT.txt`. GNU license texts and additional required notices are
retained in `assets/licenses/glibc-*.txt`. The native package, upstream source archive and
platform patch set are identified in `packaging/android/runtime.json`. The unchanged native
package, matching GNU source, platform adaptations and notices are also preserved in the
[Android runtime dependency release](https://github.com/cinnabar-mc/cinnabar/releases/tag/android-bds-runtime-20261022).
The adaptations also include Android Bionic code under BSD and Apache 2.0 and
BSD-licensed `libandroid-shmem`, with Android ashmem code under Apache 2.0. Their notices are retained in
`assets/licenses/Android-Bionic-BSD.txt`, `libandroid-shmem-BSD.txt`,
`Android-ashmem-NOTICE.txt` and `Apache-2.0.txt` in the same directory.

The translator uses Android Bionic's application-safe interface enumeration under the BSD
license. Its sources are pinned in the same manifest; the 2015 and 2016 Android Open Source
Project notices are retained in `assets/licenses/Android-Bionic-BSD.txt`.

To use a modified native runtime, build those sources with the Android adaptations and set
`CINNABAR_ANDROID_BDS_NATIVE_DIR` to the resulting library directory when rebuilding the APK.
The packager uses the source filenames and APK filenames in the manifest's `libraries` list.
This replaces the shared libraries without changing the client or translator sources.

The translated backend downloads only its required x86 GNU runtime library from the pinned
upstream revision, verifies the file's size and SHA-256, and caches it locally. It is not bundled in the APK. GNU
runtime libraries retain the GPL version 3 with the GCC Runtime Library Exception version 3.1;
the corresponding license texts are in `assets/licenses/GCC-GPL-3.0.txt` and
`assets/licenses/GCC-Runtime-Exception-3.1.txt`. Mojang's server remains a separate official download.

## Native Bedrock catalog facts

Block hardness, ordinary-tool tags, harvest requirements and recipe item tags
come from the shared [protocolgen catalog](https://github.com/cinnabar-mc/protocolgen/tree/cf01aa84b74df45df5fa24569e1ac358e4afbb87/generated/data).
Its selected native 1.26.51 measurements are from
[didntpot/bedrock-data](https://github.com/didntpot/bedrock-data), commit
`bb896a46ba856c58383e3cfb962a38c45a0dea78`, under CC0-1.0. The source lock and
upstream licence are retained in protocolgen. Sword/shears speed targets still
use the provisional Prismarine input described below; complete mining parity
remains incomplete.

## Block property and component data

`crates/assets/data/block-properties-v2193.json` and
`crates/assets/data/block-components-v2193.tsv` contain selected metadata facts
from the sample archive pinned in `assets/vanilla-source.json`. The sample files
carry the notice “(c) Mojang AB. All rights reserved.” and are subject to the
[Minecraft End User License Agreement](https://www.minecraft.net/en-us/eula).

## Gophertunnel login identifier

The self-signed login ID derivation and its regression vectors in
`crates/protocol/src/login/client_data/native_profile.rs` are adapted from
[Gophertunnel](https://github.com/HashimTheArab/gophertunnel/tree/81e65c54fb21587a6492014857710e32740c2a4f/minecraft/protocol/login).

The native profile statistics selection and aggregation in
`core/catalog/native_profile_policy.json` and
`crates/launcher-host/src/native_account/profile/statistics.rs` also follow
[Gophertunnel's userstats profile owner](https://github.com/HashimTheArab/gophertunnel/blob/81e65c54fb21587a6492014857710e32740c2a4f/minecraft/service/userstats/profile.go).
The shared conformance responses are original synthetic fixtures.

MIT License

Copyright (c) 2019 Sandertv

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Go URL, JSON and metadata semantics

The original Rust origin policy in `web/gateway/src/backend/origin.rs` follows
host parsing and unescaping rules from Go's `net/url` package. Canonical claim
alias matching and title metadata validation now live in
`bedrock-net::login::client_data`; that library retains the Go JSON and metadata
notices with its original generator. Shared conformance corpora are original
synthetic data. The source-attributed locale predicate and generated
Unicode/locale membership data in that library follow its pinned Go `unicode` and
`golang.org/x/text/internal/language` owners under the same BSD license.

Copyright 2009 The Go Authors.
Copyright 2013 The Go Authors. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

* Redistributions of source code must retain the above copyright
  notice, this list of conditions and the following disclaimer.
* Redistributions in binary form must reproduce the above
  copyright notice, this list of conditions and the following disclaimer
  in the documentation and/or other materials provided with the distribution.
* Neither the name of Google LLC nor the names of its
  contributors may be used to endorse or promote products derived from
  this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

## Native Snappy compression

The native `bedrock-net` Snappy codec adapts HashimTheArab's implementation,
including its minlz 1.2.3 encoder derived from Go's Snappy and Klaus Post's S2 1.18.1.
The following notice is retained with packaged Cinnabar resources.

Copyright 2024 Karpeles Lab Inc.
Copyright 2016 The Snappy-Go Authors. All rights reserved.
Copyright (c) 2011 The Snappy-Go Authors. All rights reserved.
Copyright (c) 2012 The Go Authors. All rights reserved.
Copyright (c) 2019 Klaus Post. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

* Redistributions of source code must retain the above copyright notice,
  this list of conditions and the following disclaimer.
* Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.
* Neither the name of Google Inc. nor the names of its contributors may be
  used to endorse or promote products derived from this software without
  specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

## Native DEFLATE encoder

Native `bedrock-net` encoding uses libdeflater and libdeflate-sys, pinned in
`Cargo.lock` and licensed under Apache-2.0. The full Apache license appears below.
Their bundled libdeflate uses the following MIT license:

Copyright 2016 Eric Biggers
Copyright 2024 Google LLC

Permission is hereby granted, free of charge, to any person
obtaining a copy of this software and associated documentation files
(the "Software"), to deal in the Software without restriction,
including without limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of the Software,
and to permit persons to whom the Software is furnished to do so,
subject to the following conditions:

The above copyright notice and this permission notice shall be
included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Native login JOSE decoding

The protected JOSE header, public JWK and certificate-chain shapes in
`bedrock-net` follow go-jose under Apache-2.0. Its certificate and time decoding
also follows the Go owners covered by the BSD notice above.

Copyright 2014 Square Inc.

## Apache-2.0 license

The following license covers libdeflater, libdeflate-sys and the attributed
go-jose implementation references.

                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright [yyyy] [name of copyright owner]

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.

## Setup window panorama

- Files: `assets/panorama/` (`panorama_0.jpg` .. `panorama_5.jpg`)
- Provenance: original, generated for Cinnabar; not Mojang content.

## Launcher title logo

- Files: `assets/branding/` (`title.png`)
- Provenance: original, generated for Cinnabar; not Mojang content.

## OreUI artwork

- Files: `assets/oreui-icons/`
- Provenance: original pixel art drawn for Cinnabar by `tools/oreui_icons/build.py`; not Mojang
  content.

## Java HUD textures

- Files: `assets/java-hud/textures/` (`hotbar_offhand_left.png`, `hotbar_offhand_right.png`,
  `cinnabar_badge.png`)
- Provenance: original pixel art drawn for Cinnabar; not Mojang content.

## Window and installer icon

- Files: `packaging/icons/cinnabar.svg`, `assets/branding/icon.png`
- Provenance: original Cinnabar artwork; not Mojang content. The PNG is rasterized
  from the SVG with `magick -background none packaging/icons/cinnabar.svg -resize 256x256 assets/branding/icon.png`.

## BedSim

- Source: https://github.com/oomph-ac/bedsim
- Commit: `28403153ffb5f8da0d170ce523d04f2c60ad58a5`
- License: MIT
- Copyright: Copyright (c) 2026 Oomph AC
- Upstream license SHA-256:
  `f9b70c22e3d853012404e41d51beae215592bbe6f94414e05c6e284c263bfacc`

<!-- BEGIN BEDSIM-MIT -->
MIT License

Copyright (c) 2026 Oomph AC

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
<!-- END BEDSIM-MIT -->

## Cinnangles fonts

The client ships `assets/fonts/CinnanglesSans.ttf`, `CinnanglesTen.ttf`,
`CinnanglesSeven.ttf`, `CinnanglesFive.ttf` and `CinnanglesFive-Bold.ttf`.
Ten, Seven, Five and Five Bold are derived from Cinnangles Sans; their source
manifests pin the exact Sans input. Sans provenance and licensing review is pending.
No license is asserted here for these faces.

## DirectX Shader Compiler

- Source: https://github.com/microsoft/DirectXShaderCompiler
- File: `dxcompiler.dll` (x64), installed beside `cinnabar.exe` by the Windows packages and
  `make play`; used unmodified. The release archive, its directory and the SHA-256 of both the
  archive and the library are pinned in `packaging/windows/dxc.json`.
- License: University of Illinois/NCSA Open Source License (LLVM Release License). The release
  notes apply it to every packaged file except `d3d12shader.h`, which Cinnabar does not ship.
- Copyright: Copyright (c) 2003-2015 University of Illinois at Urbana-Champaign; LLVM Team.
- Full license, with the notices for third-party code the compiler includes:
  `assets/licenses/DirectXShaderCompiler-LICENSE.txt`, included in packaged resources. It is the
  repository's `LICENSE.TXT` at the pinned release's tag, without the notices it marks as build
  and test dependencies only; refresh it whenever the pin changes.

## FreeType

`assetc` statically links FreeType 2.13.2, bundled by `freetype-sys` 0.22.0, to rasterize
the OreUI face bundles. Portions of this software are copyright © 2024 The FreeType Project
(www.freetype.org). All rights reserved. FreeType is used under the FreeType License (FTL);
see https://freetype.org/license.html.

## Optional outline-font fixtures

The licenses below belong to the dormant `outline-font-assets` path and its explicit
compatibility fixtures.

- Source: https://github.com/IdreesInc/Monocraft
- Commit: `e498bf70aeb25b4bdcff1e44d878fb2cb4f7c2a9`
- License: OFL-1.1
- Copyright: Copyright (c) 2022, Idrees Hassan
- Upstream license SHA-256:
  `f69c147003e052dbc9d96c40a9f73647e72766cfda95a597b94ed827fe25acb1`
- Full license: `assets/licenses/Monocraft-OFL-1.1.txt`

## Noto Sans CJK SC Regular

- Source: https://github.com/notofonts/noto-cjk
- Commit: `f8d157532fbfaeda587e826d4cd5b21a49186f7c`
- Copyright: © 2014-2021 Adobe (http://www.adobe.com/).
- License: OFL-1.1
- Exact upstream license SHA-256:
  `6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2`
- Full license: `assets/licenses/NotoSansCJK-OFL-1.1.txt`
- The optional outline-font carrier includes both source fonts' full licenses
  and copyright notices when that path is used. Source fonts remain
  unmodified in the local cache; upstream family names identify attribution.

## PMMP BedrockData

- Source: https://github.com/pmmp/BedrockData
- Tag: `6.7.0+bedrock-1.26.30`
- Commit: `bdb44a48fb6beffb6e9f6864f06d2232eb62b6a3`
- License: CC0-1.0
- Upstream license SHA-256:
  `a2010f343487d3f7618affe54f789f5487602331c0a8d03f49e9a7c547cf0499`

<!-- BEGIN PMMP-BEDROCKDATA-CC0 -->
Creative Commons Legal Code

CC0 1.0 Universal

    CREATIVE COMMONS CORPORATION IS NOT A LAW FIRM AND DOES NOT PROVIDE
    LEGAL SERVICES. DISTRIBUTION OF THIS DOCUMENT DOES NOT CREATE AN
    ATTORNEY-CLIENT RELATIONSHIP. CREATIVE COMMONS PROVIDES THIS
    INFORMATION ON AN "AS-IS" BASIS. CREATIVE COMMONS MAKES NO WARRANTIES
    REGARDING THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS
    PROVIDED HEREUNDER, AND DISCLAIMS LIABILITY FOR DAMAGES RESULTING FROM
    THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS PROVIDED
    HEREUNDER.

Statement of Purpose

The laws of most jurisdictions throughout the world automatically confer
exclusive Copyright and Related Rights (defined below) upon the creator
and subsequent owner(s) (each and all, an "owner") of an original work of
authorship and/or a database (each, a "Work").

Certain owners wish to permanently relinquish those rights to a Work for
the purpose of contributing to a commons of creative, cultural and
scientific works ("Commons") that the public can reliably and without fear
of later claims of infringement build upon, modify, incorporate in other
works, reuse and redistribute as freely as possible in any form whatsoever
and for any purposes, including without limitation commercial purposes.
These owners may contribute to the Commons to promote the ideal of a free
culture and the further production of creative, cultural and scientific
works, or to gain reputation or greater distribution for their Work in
part through the use and efforts of others.

For these and/or other purposes and motivations, and without any
expectation of additional consideration or compensation, the person
associating CC0 with a Work (the "Affirmer"), to the extent that he or she
is an owner of Copyright and Related Rights in the Work, voluntarily
elects to apply CC0 to the Work and publicly distribute the Work under its
terms, with knowledge of his or her Copyright and Related Rights in the
Work and the meaning and intended legal effect of CC0 on those rights.

1. Copyright and Related Rights. A Work made available under CC0 may be
protected by copyright and related or neighboring rights ("Copyright and
Related Rights"). Copyright and Related Rights include, but are not
limited to, the following:

  i. the right to reproduce, adapt, distribute, perform, display,
     communicate, and translate a Work;
 ii. moral rights retained by the original author(s) and/or performer(s);
iii. publicity and privacy rights pertaining to a person's image or
     likeness depicted in a Work;
 iv. rights protecting against unfair competition in regards to a Work,
     subject to the limitations in paragraph 4(a), below;
  v. rights protecting the extraction, dissemination, use and reuse of data
     in a Work;
 vi. database rights (such as those arising under Directive 96/9/EC of the
     European Parliament and of the Council of 11 March 1996 on the legal
     protection of databases, and under any national implementation
     thereof, including any amended or successor version of such
     directive); and
vii. other similar, equivalent or corresponding rights throughout the
     world based on applicable law or treaty, and any national
     implementations thereof.

2. Waiver. To the greatest extent permitted by, but not in contravention
of, applicable law, Affirmer hereby overtly, fully, permanently,
irrevocably and unconditionally waives, abandons, and surrenders all of
Affirmer's Copyright and Related Rights and associated claims and causes
of action, whether now known or unknown (including existing as well as
future claims and causes of action), in the Work (i) in all territories
worldwide, (ii) for the maximum duration provided by applicable law or
treaty (including future time extensions), (iii) in any current or future
medium and for any number of copies, and (iv) for any purpose whatsoever,
including without limitation commercial, advertising or promotional
purposes (the "Waiver"). Affirmer makes the Waiver for the benefit of each
member of the public at large and to the detriment of Affirmer's heirs and
successors, fully intending that such Waiver shall not be subject to
revocation, rescission, cancellation, termination, or any other legal or
equitable action to disrupt the quiet enjoyment of the Work by the public
as contemplated by Affirmer's express Statement of Purpose.

3. Public License Fallback. Should any part of the Waiver for any reason
be judged legally invalid or ineffective under applicable law, then the
Waiver shall be preserved to the maximum extent permitted taking into
account Affirmer's express Statement of Purpose. In addition, to the
extent the Waiver is so judged Affirmer hereby grants to each affected
person a royalty-free, non transferable, non sublicensable, non exclusive,
irrevocable and unconditional license to exercise Affirmer's Copyright and
Related Rights in the Work (i) in all territories worldwide, (ii) for the
maximum duration provided by applicable law or treaty (including future
time extensions), (iii) in any current or future medium and for any number
of copies, and (iv) for any purpose whatsoever, including without
limitation commercial, advertising or promotional purposes (the
"License"). The License shall be deemed effective as of the date CC0 was
applied by Affirmer to the Work. Should any part of the License for any
reason be judged legally invalid or ineffective under applicable law, such
partial invalidity or ineffectiveness shall not invalidate the remainder
of the License, and in such case Affirmer hereby affirms that he or she
will not (i) exercise any of his or her remaining Copyright and Related
Rights in the Work or (ii) assert any associated claims and causes of
action with respect to the Work, in either case contrary to Affirmer's
express Statement of Purpose.

4. Limitations and Disclaimers.

 a. No trademark or patent rights held by Affirmer are waived, abandoned,
    surrendered, licensed or otherwise affected by this document.
 b. Affirmer offers the Work as-is and makes no representations or
    warranties of any kind concerning the Work, express, implied,
    statutory or otherwise, including without limitation warranties of
    title, merchantability, fitness for a particular purpose, non
    infringement, or the absence of latent or other defects, accuracy, or
    the present or absence of errors, whether or not discoverable, all to
    the greatest extent permissible under applicable law.
 c. Affirmer disclaims responsibility for clearing rights of other persons
    that may apply to the Work or any use thereof, including without
    limitation any person's Copyright and Related Rights in the Work.
    Further, Affirmer disclaims responsibility for obtaining any necessary
    consents, permissions or other rights required for any use of the
    Work.
 d. Affirmer understands and acknowledges that Creative Commons is not a
    party to this document and has no duty or obligation with respect to
    this CC0 or use of the Work.
<!-- END PMMP-BEDROCKDATA-CC0 -->

## PrismarineJS minecraft-data

- Source: https://github.com/PrismarineJS/minecraft-data
- Commit: `6ec59288287e4045331eaa47ee8fb104278f6b98`
- License declaration: MIT
- License evidence: the pinned `README.md`, SHA-256
  `c0b6a32a38ac3070c908eba107b25e14385a4d8534ada08780956748da589561`

The pinned tree does not contain a standalone LICENSE, LICENCE, COPYING, or NOTICE file.
Its root `README.md` explicitly declares MIT and also notes that individual
upstream-derived data may require different terms. The full MIT terms below are
retained for the PrismarineJS/minecraft-data contributors without inventing a
person or copyright year.

<!-- BEGIN PRISMARINEJS-MINECRAFT-DATA-MIT -->
MIT License

Copyright PrismarineJS/minecraft-data contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
<!-- END PRISMARINEJS-MINECRAFT-DATA-MIT -->

## GeyserMC mappings

- Source: https://github.com/GeyserMC/mappings
- Commit: `81700ab543c1f4e9f9041f58dac4154ecb0d0c8b` (Java Edition 26.3, Bedrock Edition 1.26.50)
- Archive: the `2.1.0-build.1` release of https://github.com/GeyserMC/mappings-generator,
  the generator's published output for that commit, pinned in
  `assets/geyser-mappings-source.json`
- License: MIT
- Copyright: Copyright (c) 2019-2026 GeyserMC
- Upstream license SHA-256:
  `4f49de5134b13de98672e8f870aae5d36f5b415086fb1c6fdf832b78c47c7094`
- Use: `crates/assets/data/java-mapping-v2193.bin` is generated from its Java-to-Bedrock
  block-state, biome and item mappings and from the archive's MCProtocolLib network exports
  (`mcpl/data/networkTags.nbt` block and biome tags, `mcpl/data/networkCodec.nbt` Java biome
  registry). GeyserMC licenses only its contributors' own work; it does not relicense
  Minecraft data owned by Mojang or Microsoft.

<!-- BEGIN GEYSERMC-MAPPINGS-MIT -->
The MIT License

Copyright (c) 2019-2026 GeyserMC. http://geysermc.org

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
<!-- END GEYSERMC-MAPPINGS-MIT -->

## GeyserMC Geyser

- Source: https://github.com/GeyserMC/Geyser
- Commit: `8f6a9d20b05c302282f9496314b7c18cf6d4cc53`, pinned in
  `assets/geyser-mappings-source.json`
- License: MIT
- Copyright: Copyright (c) 2019-2025 GeyserMC
- Upstream license SHA-256:
  `7380cf017734f26977e907a07fd806e9dcba38023d522b7c6e56bd3a2482b08a`
- Use: the Java entity type to Bedrock entity id pairs in
  `crates/assets/data/java-mapping-v2193.bin` are read from its `VanillaEntities.java` and
  `BedrockEntityDefinitions.java`.

<!-- BEGIN GEYSERMC-GEYSER-MIT -->
The MIT License

Copyright (c) 2019-2025 GeyserMC. http://geysermc.org

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
<!-- END GEYSERMC-GEYSER-MIT -->

## Axolotl Stack

- Source: https://github.com/axolotl-stack/axolotl-stack
- Commit: `6f6806e821a579c183c44d786f76d9b358a2b825`
- License: MIT
- Copyright: Copyright (c) 2025 Jeremy Ianne
- Upstream license SHA-256:
  `62c75fcb256604584191434b605dc3fe661d938a94b2c35836ef55011bf24184`
- Components: Jolyne (client session library), whose raw packet entries and spawn
  sequence are adapted in `crates/protocol/src/raw_packet.rs` and
  `crates/protocol/src/login/startup.rs`. Current revision:
  `c4540512dc47833bb40363da7ad1161110d64b67`, as recorded in
  `crates/protocol/vendor/UPSTREAM.md`.

<!-- BEGIN AXOLOTL-STACK-MIT -->
MIT License

Copyright (c) 2025 Jeremy Ianne

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.<!-- END AXOLOTL-STACK-MIT -->

## Dragonfly

- Source: https://github.com/df-mc/dragonfly
- Commit: `b85c56ffea6b306798a935f14cc941c76618be52`
- License: MIT
- Copyright: Copyright (c) 2019 Dragonfly Tech
- Upstream license SHA-256:
  `9b0866098f4b7bfadafa43adec71dae35968053ceaea0487fb4b23c46cc72755`
- Earlier item-tag tables were derived from the `hashimthearab/dragonfly` fork at
  `3d29a693c54b8412a6a1c619f5b06bd8cb09a0e5` under the same license.

<!-- BEGIN DRAGONFLY-MIT -->
MIT License

Copyright (c) 2019 Dragonfly Tech

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
<!-- END DRAGONFLY-MIT -->

## redb persistent blob storage

- Source: https://github.com/cberner/redb
- Dependency: `redb`, pinned in `Cargo.lock`.
- License: MIT OR Apache-2.0; Cinnabar distributes it under the MIT option.
- Copyright: 2021 Christopher Berner.
- Full selected license: `assets/licenses/redb-MIT.txt`, included in packaged resources.

## tracing subscriber stack

- Source: https://github.com/tokio-rs/tracing
- Dependencies: `tracing-subscriber`, `tracing-log` and `tracing-error`, pinned in `Cargo.lock`;
  `diagnostics::logging` builds the process subscriber from them. Bevy's log plugin linked the
  same crates before it was replaced.
- License: MIT.
- Copyright: Copyright (c) Tokio Contributors.
- Android builds add `android_log-sys` (https://github.com/rust-mobile/android_log-sys-rs),
  MIT OR Apache-2.0, distributed under the MIT option. The `frame-trace` developer feature
  adds `tracing-chrome` (https://github.com/thoren-d/tracing-chrome), MIT; release builds do
  not include it.

## Crash capture and minidumps

- Source: https://github.com/EmbarkStudios/crash-handling and
  https://github.com/rust-minidump/minidump-writer
- Dependencies: `crash-handler`, `crash-context`, `minidumper` and `minidump-writer`, pinned in
  `Cargo.lock`; the client installs the handler and the `--crash-monitor` process writes dumps.
  Tests add `sadness-generator` from the same repository.
- License: MIT OR Apache-2.0; Cinnabar distributes them under the MIT option.
- Copyright: Copyright (c) 2022 Embark Studios; rust-minidump contributors.
- `minidump` (https://github.com/rust-minidump/rust-minidump), MIT, reads the fault location
  from each dump for the crash window.
- `memmap2` (https://github.com/RazrFalcon/memmap2-rs), MIT OR Apache-2.0 under the MIT
  option, Copyright (c) 2020 Yevhenii Reizner and Copyright (c) 2015 Dan Burkert, maps the
  heartbeat file the client and its crash monitor share for hang detection.

## Gophertunnel numeric contracts

The protocol ID fixture uses numeric constants from Gophertunnel, under the MIT license.

MIT License

Copyright (c) 2019 Sandertv

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Crash symbolication tooling

- Source: https://github.com/rust-minidump/rust-minidump, https://github.com/mstange/samply and
  https://github.com/luser/rust-test-assembler
- Dependencies: `minidump`, `minidump-processor` and `minidump-unwind` (MIT) and `wholesym`
  (MIT OR Apache-2.0) in `tools/devtool`, with the test-only `minidump-synth` and
  `test-assembler` (MIT); pinned in `Cargo.lock`, used unmodified.
- Transitive licenses: MIT, Apache-2.0, BSD-3-Clause, NCSA (`msvc-demangler`, MIT/NCSA), ISC
  (`untrusted`, `rustls-webpki`; `ring` is Apache-2.0 AND ISC), CDLA-Permissive-2.0
  (`webpki-roots`), Unicode-3.0 (`yoke`) and MPL-2.0 (`option-ext`). `wholesym` always links an
  HTTP client for symbol servers; `devtool symbolicate` configures none and never uses it.
- Distribution: none. `devtool symbolicate` is a developer tool and is not packaged.

## Inter

- File: `assets/fonts/InterVariable.ttf`
- Source: https://github.com/rsms/inter
- Version: 4.1, commit `e3a3d4c57d5ecc01453a575621882a384c1995a3`
- Copyright (c) 2016 The Inter Project Authors (https://github.com/rsms/inter)
- License: SIL Open Font License 1.1; full text in `assets/licenses/Inter-OFL-1.1.txt`.
- Font SHA-256: `4989b125924991b90d05b2d16e0e388c48f7d5bb8b30539bbf9c755278d0ccaf`

Authored native panels may select Inter with `font_type: "mod_panel_sans"`.
The cosmetic regression geometry and textures in `crates/render-model/src/hats/` are original Cinnabar artwork, distributed under the repository license. Editable projects and production catalog assets are maintained in cinnabar-mc/cinnabar-api.

## pp-rs shader preprocessor

- Source: https://github.com/Kangz/glslpp-rs
- Cinnabar fork: https://github.com/cinnabar-mc/glslpp-rs, pinned in `Cargo.lock`.
- Dependency: `pp-rs`, shared by properties and shader preprocessing.
- License: BSD-3-Clause.
- The fork adds opt-in Iris conditions and text output, and bounds expression recursion.

BSD 3-Clause License

Copyright (c) 2020, Corentin Wallez
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from
   this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

## Android platform certificate verification

Android packages the JVM component from [rustls-platform-verifier](https://github.com/rustls/rustls-platform-verifier),
under the MIT license retained in `packaging/android/licenses/rustls-platform-verifier-LICENSE-MIT`.
Its original network-security resource supports certificate revocation checks.
The checked artifact/version owner is `packaging/android/platform-verifier.json`; Cargo
version drift fails packaging rather than selecting an unrelated JVM component.

The Kotlin standard library and its JDK7 compatibility artifact use Apache-2.0.
The original upstream copyright, license and notice texts are retained in
`packaging/android/licenses/kotlin-{COPYRIGHT,LICENSE,NOTICE}.txt`.
JetBrains annotations 13.0 are Copyright 2000–2012 JetBrains s.r.o., under Apache-2.0;
the source license header is retained in
`packaging/android/licenses/annotations-13.0-source-license-header.txt`.
These notices are also included as APK assets. No JVM binaries are committed here.

## Native RakNet transport

Cinnabar RakNet modifications
Copyright 2026 Cinnabar contributors

The client MTU negotiation state machine follows public Lunar go-raknet behavior
at commit 6124f52657d4d5f006ea2af2d525260998dd3173. It adapts its probe scheduling,
protected challenge, cookie refresh and final-grant handling to caller-driven
Rust protocol state and explicit socket-write completion inputs. Client system
addresses, mapped IPv4 normalization and IPv6 wire fields also follow this
reference. System-address codecs retain short-list support and reject excess
entries with bounded input allocation. UDP error classification follows the
reference's Unix and Winsock handshake read policies.

Receive-window advancement and recent-loss selection also follow this reference.
ACK/NACK emission remains caller-driven, with bounded pending loss records.

That reference is MIT-licensed, copyright 2019 Sander ten Veldhuis.
See LICENSE-MIT-go-raknet for the retained notice and license.
The original RakNet project's Apache-2.0 license and notices remain in effect.

Early application-packet retention and connected liveness controls follow the
same public behavior reference, with explicit Rust-owned handshake bounds and
ordered transfer of pending packets and terminal notifications.

Default client GUID generation uses fresh OS entropy for each ping/connection,
with the reference's little-endian payload mapping and client type bits. Identity
is supplied explicitly to the Rust engine and retained in attempt metadata.

Wire timestamps use endpoint-relative monotonic uptime, following the same
reference's timestamp and remote SystemUptime behavior. Portable Rust snapshots
rebase their timer offsets using caller-supplied transfer elapsed time.

MIT License

Copyright (c) 2019 Sander ten Veldhuis

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## AMD FidelityFX Super Resolution 1

- Source: https://github.com/GPUOpen-Effects/FidelityFX-FSR
- License: MIT.
- Use: FP32 EASU, RCAS and reversible HDR colour transforms from `ffx-fsr/ffx_fsr1.h`,
  ported to `crates/render/src/world_upscale.wgsl`; CPU setup in
  `crates/render/src/ui_render/upscale.rs`.

Copyright (c) 2021 Advanced Micro Devices, Inc. All rights reserved.
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files(the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and / or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions :
The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.
THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
