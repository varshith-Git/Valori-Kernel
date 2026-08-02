"use client";

import { use } from "react";
import { OperationDetailView } from "@/components/operations/OperationDetailView";

export default function OperationDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = use(params);
  return <OperationDetailView operationId={id} />;
}
