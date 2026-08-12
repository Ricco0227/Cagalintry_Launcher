import { ExternalLink, ShieldCheck, UserPlus } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Button } from "@/components/Button";
import { Section } from "@/components/Field";
import { Page } from "@/components/Page";

/**
 * Accounts.
 *
 * Two separate identities live here and the page keeps them visibly separate:
 * a Cagalintry account, which is who you are to the sync server and who owns
 * the packs you publish; and one or more Minecraft accounts, which is what
 * actually launches the game. Neither is usable yet — Cagalintry accounts
 * arrive with the sync server, Microsoft sign-in with Phase 2.
 */
export function Accounts() {
  return (
    <Page title="Accounts" subtitle="Cagalintry and Minecraft accounts">
      <div className="flex max-w-[620px] flex-col gap-5">
        <Section
          title="Minecraft"
          description="Signing in with a Microsoft account that owns Minecraft is what lets you play."
        >
          <div className="flex items-center gap-3 rounded-[12px] border border-dashed border-border-strong px-4 py-5">
            <div className="grid size-10 shrink-0 place-items-center rounded-[10px] bg-surface-2 text-text-subtle">
              <UserPlus size={19} />
            </div>
            <div className="min-w-0 flex-1">
              <p className="text-[13.5px] font-medium">No Minecraft account linked</p>
              <p className="mt-0.5 text-[12.5px] leading-relaxed text-text-muted">
                Sign-in arrives in the next phase, once the launcher&rsquo;s Microsoft application
                is approved for the Minecraft API.
              </p>
            </div>
            <Button variant="primary" disabled>
              Sign in
            </Button>
          </div>

          <div className="flex items-start gap-2.5 rounded-[12px] bg-surface-2 px-3.5 py-3">
            <ShieldCheck size={16} className="mt-px shrink-0 text-success" />
            <p className="text-[12.5px] leading-relaxed text-text-muted">
              A Microsoft account that owns the game is the only way in. Ownership is verified
              against Mojang before a session can be used, and there is no offline mode.
            </p>
          </div>
        </Section>

        <Section
          title="Cagalintry"
          description="Your identity on the sync server: who owns a pack, and who may publish to it."
        >
          <div className="flex items-center gap-3 rounded-[12px] border border-dashed border-border-strong px-4 py-5">
            <div className="min-w-0 flex-1">
              <p className="text-[13.5px] font-medium">Not connected to a sync server</p>
              <p className="mt-0.5 text-[12.5px] leading-relaxed text-text-muted">
                Accounts are created by an administrator — there is no self-signup. Once a server
                address is configured you can sign in here and link your Minecraft account to it.
              </p>
            </div>
            <Button disabled>Sign in</Button>
          </div>
        </Section>

        <p className="px-1 text-[12px] text-text-subtle">
          Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.{" "}
          <button
            type="button"
            onClick={() => void openUrl("https://www.minecraft.net/en-us/usage-guidelines")}
            className="inline-flex items-center gap-1 text-accent hover:underline"
          >
            Usage guidelines
            <ExternalLink size={11} />
          </button>
        </p>
      </div>
    </Page>
  );
}
