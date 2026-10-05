import { constructWebhook } from "@mayarin/sdk";
import { createFileRoute } from "@tanstack/react-router";
import { settleCycle } from "../../../server/mayarin";

type PaymentEvent = {
  id: string;
  type: string;
  data?: {
    paymentIntentId?: string;
    state?: string;
    metadata?: { subscriptionId?: string; invoiceId?: string };
  };
};

/**
 * Mayarin webhooks (https://docs.mayarin.xyz/guides/webhooks). Register this
 * URL on the Mayarin dashboard: POST /api/webhooks/mayarin. A 2xx stops
 * retries; a thrown error answers 500 and Mayarin retries later.
 */
export const Route = createFileRoute("/api/webhooks/mayarin")({
  server: {
    handlers: {
      POST: async ({ request }) => {
        const secret = process.env.MAYARIN_WEBHOOK_SECRET;
        if (!secret) return new Response("Webhooks are not configured", { status: 503 });

        let event: PaymentEvent;
        try {
          // The raw body: re-serialized JSON would not match the signature.
          event = await constructWebhook<PaymentEvent>({
            payload: await request.text(),
            signature: request.headers.get("webhook-signature") ?? "",
            secret,
          });
        } catch {
          return new Response(null, { status: 401 });
        }

        const subscriptionId = event.data?.metadata?.subscriptionId;
        const paymentIntentId = event.data?.paymentIntentId;
        if (event.type === "payment.state_changed" && subscriptionId && paymentIntentId) {
          await settleCycle({
            subscriptionId,
            paymentIntentId,
            invoiceId: event.data?.metadata?.invoiceId,
          });
        }
        return new Response(null, { status: 204 });
      },
    },
  },
});
