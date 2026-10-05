<?--
This file is part of Privatium
apps/_lint/pass/PV111/pv111ok/views/index.lsp
Summary: The one view: a heading, a sentence and a link; no script and no stylesheet of its own.
Notes: See README file for documentation and full license information.
--?>

<h1>Lint</h1>
<p>The frame's head loads the stylesheet and the script named in app.toml, once.</p>
<p><a href="<?= url('/more') ?>">More</a></p>
