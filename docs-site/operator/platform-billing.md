# Platform billing

The gateway bills each project for using it, through its own payment and provider code.

## Plans

Two plans exist after the migration: `Test` (free, for untrusted projects) and `Live` (for trusted projects). Every price is zero until you set one, so nothing is invoiced by default. Edit a plan with `POST /console/api/admin/platform/plans/{id}` or create one with `POST /console/api/admin/platform/plans`. A plan has a monthly price, an included volume, a percent fee in basis points, a base fee per settled payment and optional limits that layer under the per project overrides.

## Who gets paid

Pick the platform owned project with `POST /console/api/admin/platform/tenant` (`{"tenant_id": 1}`). Its providers collect the invoices. Until it is set, invoices are created but no payment is attached.

## Metering and invoices

Usage is the count and volume of payments settled in each monthly period. The period invoice carries the transaction fee, the volume fee above the included volume and next month's plan fee. Upgrades are prorated and charged at once. Downgrades become credit applied to the next invoice. Projects see invoices on the Billing tab, with the pay link, the receipt and the method used (card or Lightning).

## Past due

An unpaid invoice is due after 3 days. A project is warned when overdue, restricted from 7 days overdue (new payments are refused, refunds and withdrawals work), and disabled from 21 days (new attempts are refused as well). Withdrawals, refunds and settlement of payments created earlier are never blocked. Each change is audit logged as `platform.standing_changed` and mailed to the project owners. Paying or waiving restores service.

## Overrides

Operators can change a project's plan, grant credit and waive an invoice, each with a required reason: `POST /console/api/admin/projects/{id}/platform/plan`, `.../platform/credit`, `.../platform/invoices/{invoice}/waive`. All are audit logged.
